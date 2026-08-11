#![forbid(unsafe_code)]

use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc, LazyLock,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use wait_timeout::ChildExt;

const GIT_COMMAND_TIMEOUT: Duration = Duration::from_millis(100);
const CAPTURE_IDENTITY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_IDENTITY_WORKERS: usize = 4;
static IDENTITY_WORKERS: LazyLock<WorkerPool> =
    LazyLock::new(|| WorkerPool::new(MAX_IDENTITY_WORKERS));

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ProjectOriginKind {
    Git,
    Directory,
    Unresolved,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectOriginSnapshot {
    pub identity: String,
    pub kind: ProjectOriginKind,
    pub display_path: PathBuf,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ProjectCaptureSnapshot {
    pub submitted_cwd: PathBuf,
    pub origin: ProjectOriginSnapshot,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIdentity {
    key: String,
    display_path: PathBuf,
    worktree_path: PathBuf,
}

impl ProjectIdentity {
    pub fn from_cwd(cwd: &Path) -> Result<Self, IdentityError> {
        let (_, key_path, display_path, worktree_path) =
            canonical_project_paths(cwd, Path::new("git"))?;
        Ok(Self {
            key: project_key(&key_path),
            display_path,
            worktree_path,
        })
    }

    pub fn capture_snapshot_from_cwd(cwd: &Path) -> Result<ProjectCaptureSnapshot, IdentityError> {
        capture_snapshot_bounded(cwd, PathBuf::from("git"), CAPTURE_IDENTITY_TIMEOUT)
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }

    pub fn display_path(&self) -> &Path {
        &self.display_path
    }
}

fn capture_snapshot_bounded(
    cwd: &Path,
    git_program: PathBuf,
    timeout: Duration,
) -> Result<ProjectCaptureSnapshot, IdentityError> {
    capture_snapshot_bounded_with_pool(cwd, git_program, timeout, &IDENTITY_WORKERS)
}

fn capture_snapshot_bounded_with_pool(
    cwd: &Path,
    git_program: PathBuf,
    timeout: Duration,
    workers: &WorkerPool,
) -> Result<ProjectCaptureSnapshot, IdentityError> {
    let submitted_cwd = cwd.to_path_buf();
    let fallback = unresolved_snapshot(&submitted_cwd);
    let Some(permit) = workers.try_acquire() else {
        return Ok(fallback);
    };
    let worker_cwd = submitted_cwd.clone();
    let (sender, receiver) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("akra-identity".to_owned())
        .spawn(move || {
            let _permit = permit;
            let result = capture_snapshot(&worker_cwd, &git_program);
            let _ = sender.send(result);
        })
        .map_err(IdentityError::WorkerSpawn)?;
    match receiver.recv_timeout(timeout) {
        Ok(Ok(snapshot)) => Ok(snapshot),
        Ok(Err(_)) | Err(mpsc::RecvTimeoutError::Timeout) => Ok(fallback),
        Err(mpsc::RecvTimeoutError::Disconnected) => Err(IdentityError::WorkerDisconnected),
    }
}

struct WorkerPool {
    inner: Arc<WorkerPoolInner>,
}

struct WorkerPoolInner {
    active: AtomicUsize,
    limit: usize,
}

impl WorkerPool {
    fn new(limit: usize) -> Self {
        Self {
            inner: Arc::new(WorkerPoolInner {
                active: AtomicUsize::new(0),
                limit,
            }),
        }
    }

    fn try_acquire(&self) -> Option<WorkerPermit> {
        self.inner
            .active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |active| {
                (active < self.inner.limit).then_some(active + 1)
            })
            .ok()?;
        Some(WorkerPermit {
            inner: Arc::clone(&self.inner),
        })
    }
}

struct WorkerPermit {
    inner: Arc<WorkerPoolInner>,
}

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.inner.active.fetch_sub(1, Ordering::Release);
    }
}

fn capture_snapshot(
    cwd: &Path,
    git_program: &Path,
) -> Result<ProjectCaptureSnapshot, IdentityError> {
    let submitted_cwd = cwd.to_path_buf();
    let origin = match canonical_project_paths(cwd, git_program) {
        Ok((kind, key_path, display_path, _)) => ProjectOriginSnapshot {
            identity: project_key(&key_path),
            kind,
            display_path: snapshot_display_path(&display_path),
        },
        Err(error) => return Err(error),
    };
    Ok(ProjectCaptureSnapshot {
        submitted_cwd,
        origin,
    })
}

fn unresolved_snapshot(cwd: &Path) -> ProjectCaptureSnapshot {
    ProjectCaptureSnapshot {
        submitted_cwd: cwd.to_path_buf(),
        origin: ProjectOriginSnapshot {
            identity: unresolved_key(cwd),
            kind: ProjectOriginKind::Unresolved,
            display_path: cwd.to_path_buf(),
        },
    }
}

fn canonical_project_paths(
    cwd: &Path,
    git_program: &Path,
) -> Result<(ProjectOriginKind, PathBuf, PathBuf, PathBuf), IdentityError> {
    let worktree_path = cwd.canonicalize().map_err(IdentityError::Canonicalize)?;
    match git_project_paths(&worktree_path, git_program)? {
        Some((common_dir, display_path)) => Ok((
            ProjectOriginKind::Git,
            common_dir,
            display_path,
            worktree_path,
        )),
        None => Ok((
            ProjectOriginKind::Directory,
            worktree_path.clone(),
            worktree_path.clone(),
            worktree_path,
        )),
    }
}

fn project_key(path: &Path) -> String {
    hex::encode(Sha256::digest(path.to_string_lossy().as_bytes()))
}

fn unresolved_key(cwd: &Path) -> String {
    format!("unresolved:{}", project_key(cwd))
}

#[cfg(windows)]
fn snapshot_display_path(path: &Path) -> PathBuf {
    let display = path.to_string_lossy();
    if let Some(path) = display.strip_prefix(r"\\?\UNC\") {
        PathBuf::from(format!(r"\\{path}"))
    } else if let Some(path) = display.strip_prefix(r"\\?\") {
        PathBuf::from(path)
    } else {
        path.to_path_buf()
    }
}

#[cfg(not(windows))]
fn snapshot_display_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

fn git_project_paths(
    cwd: &Path,
    git_program: &Path,
) -> Result<Option<(PathBuf, PathBuf)>, IdentityError> {
    let mut command = Command::new(git_program);
    command
        .args([
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--show-toplevel",
        ])
        .current_dir(cwd);
    let output = run_command_bounded(command, GIT_COMMAND_TIMEOUT)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).to_ascii_lowercase();
        if stderr.contains("not a git repository") {
            return Ok(None);
        }
        return Err(IdentityError::GitCommandFailed(stderr.trim().to_owned()));
    }
    let paths = String::from_utf8(output.stdout).map_err(IdentityError::GitOutput)?;
    let mut paths = paths.lines().filter(|path| !path.trim().is_empty());
    let Some(common_dir) = paths.next() else {
        return Err(IdentityError::GitCommandFailed(
            "git returned no common directory".to_owned(),
        ));
    };
    let Some(display_path) = paths.next() else {
        return Err(IdentityError::GitCommandFailed(
            "git returned no worktree root".to_owned(),
        ));
    };
    Ok(Some((
        PathBuf::from(common_dir)
            .canonicalize()
            .map_err(IdentityError::GitCommonDir)?,
        PathBuf::from(display_path)
            .canonicalize()
            .map_err(IdentityError::GitCommonDir)?,
    )))
}

struct BoundedOutput {
    status: std::process::ExitStatus,
    stdout: Vec<u8>,
    stderr: Vec<u8>,
}

fn run_command_bounded(
    mut command: Command,
    timeout: Duration,
) -> Result<BoundedOutput, IdentityError> {
    let mut child = command
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(IdentityError::GitLaunch)?;
    let status = match child
        .wait_timeout(timeout)
        .map_err(IdentityError::GitWait)?
    {
        Some(status) => status,
        None => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(IdentityError::GitTimeout(timeout));
        }
    };
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    child
        .stdout
        .take()
        .ok_or(IdentityError::MissingGitPipe)?
        .read_to_end(&mut stdout)
        .map_err(IdentityError::GitOutputRead)?;
    child
        .stderr
        .take()
        .ok_or(IdentityError::MissingGitPipe)?
        .read_to_end(&mut stderr)
        .map_err(IdentityError::GitOutputRead)?;
    Ok(BoundedOutput {
        status,
        stdout,
        stderr,
    })
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("failed to canonicalize working directory: {0}")]
    Canonicalize(#[source] std::io::Error),
    #[error("failed to launch Git identity command: {0}")]
    GitLaunch(#[source] std::io::Error),
    #[error("failed while waiting for Git identity command: {0}")]
    GitWait(#[source] std::io::Error),
    #[error("Git identity command exceeded {0:?}")]
    GitTimeout(Duration),
    #[error("Git identity command failed: {0}")]
    GitCommandFailed(String),
    #[error("Git identity command returned invalid UTF-8: {0}")]
    GitOutput(#[source] std::string::FromUtf8Error),
    #[error("failed to canonicalize Git common directory: {0}")]
    GitCommonDir(#[source] std::io::Error),
    #[error("Git identity command pipe was unavailable")]
    MissingGitPipe,
    #[error("failed to read Git identity command output: {0}")]
    GitOutputRead(#[source] std::io::Error),
    #[error("capture identity worker disconnected")]
    WorkerDisconnected,
    #[error("failed to start capture identity worker: {0}")]
    WorkerSpawn(#[source] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn git_launch_failure_is_preserved_as_unresolved_capture_provenance() {
        let directory = TempDir::new().expect("working directory");
        let snapshot = capture_snapshot_bounded(
            directory.path(),
            directory.path().join("missing-git"),
            Duration::from_millis(50),
        )
        .expect("capture fallback");

        assert_eq!(snapshot.origin.kind, ProjectOriginKind::Unresolved);
        assert_eq!(snapshot.origin.display_path, directory.path());
        assert!(snapshot.origin.identity.starts_with("unresolved:"));
    }

    #[test]
    fn child_process_execution_has_a_hard_timeout() {
        #[cfg(windows)]
        let command = {
            let mut command = Command::new("cmd");
            command.args(["/C", "ping -n 6 127.0.0.1 >NUL"]);
            command
        };
        #[cfg(not(windows))]
        let command = {
            let mut command = Command::new("sh");
            command.args(["-c", "while :; do :; done"]);
            command
        };

        assert!(matches!(
            run_command_bounded(command, Duration::from_millis(20)),
            Err(IdentityError::GitTimeout(_))
        ));
    }

    #[test]
    fn saturated_identity_pool_returns_unresolved_without_spawning() {
        let workers = WorkerPool::new(2);
        let first = workers.try_acquire().expect("first permit");
        let second = workers.try_acquire().expect("second permit");
        let directory = TempDir::new().expect("working directory");

        let snapshot = capture_snapshot_bounded_with_pool(
            directory.path(),
            PathBuf::from("git"),
            Duration::from_secs(1),
            &workers,
        )
        .expect("saturated fallback");

        assert_eq!(snapshot.origin.kind, ProjectOriginKind::Unresolved);
        assert_eq!(workers.inner.active.load(Ordering::Acquire), 2);
        drop(first);
        assert!(workers.try_acquire().is_some());
        drop(second);
    }
}
