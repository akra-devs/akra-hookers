#![forbid(unsafe_code)]

use std::{
    fs::{self, File},
    io::{self, ErrorKind, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use wait_timeout::ChildExt;

const GIT_COMMAND_TIMEOUT: Duration = Duration::from_millis(100);
const WSL_GIT_COMMAND_TIMEOUT: Duration = Duration::from_millis(750);
const MAX_WSL_DISTRO_BYTES: usize = 128;
const MAX_WSL_PATH_BYTES: usize = 4096;
const MAX_WSL_ORIGIN_OUTPUT_BYTES: usize = 16 * 1024;
const MAX_GIT_POINTER_BYTES: u64 = 4 * 1024;

const WSL_ORIGIN_SCRIPT: &str = r#"cwd=$1
[ -d "$cwd" ] || exit 3
if common=$(git -C "$cwd" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) \
    && root=$(git -C "$cwd" rev-parse --path-format=absolute --show-toplevel 2>/dev/null); then
    printf 'git\000%s\000%s\000' "$common" "$root"
else
    canonical=$(CDPATH= cd -P -- "$cwd" && pwd -P) || exit 3
    printf 'directory\000%s\000' "$canonical"
fi"#;

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
        Ok(capture_snapshot_or_unresolved(cwd, Path::new("git")))
    }

    /// Captures project identity in the originating WSL distribution. The Linux
    /// path is never passed through Windows Git, avoiding UNC ownership checks
    /// and preserving linked-worktree common-directory identity.
    #[cfg(windows)]
    pub fn capture_snapshot_from_wsl(
        distro: &str,
        cwd: &str,
    ) -> Result<ProjectCaptureSnapshot, IdentityError> {
        let context = WslCaptureContext::new(distro, cwd)?;
        let fallback = context.unresolved_snapshot();
        let Some(wsl_executable) = windows_wsl_executable() else {
            return Ok(fallback);
        };
        Ok(capture_wsl_snapshot_or_unresolved(context, &wsl_executable))
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

#[derive(Clone, Debug)]
struct WslCaptureContext {
    distro: String,
    identity_distro: String,
    cwd: String,
}

impl WslCaptureContext {
    fn new(distro: &str, cwd: &str) -> Result<Self, IdentityError> {
        validate_wsl_distro(distro)?;
        validate_linux_absolute_path(cwd)?;
        Ok(Self {
            distro: distro.to_owned(),
            identity_distro: distro.to_lowercase(),
            cwd: cwd.to_owned(),
        })
    }

    fn unresolved_snapshot(&self) -> ProjectCaptureSnapshot {
        ProjectCaptureSnapshot {
            submitted_cwd: PathBuf::from(&self.cwd),
            origin: ProjectOriginSnapshot {
                identity: format!(
                    "unresolved:{}",
                    wsl_project_key(&self.identity_distro, &self.cwd)
                ),
                kind: ProjectOriginKind::Unresolved,
                display_path: wsl_display_path(&self.distro, &self.cwd),
            },
        }
    }
}

fn capture_snapshot_or_unresolved(cwd: &Path, git_program: &Path) -> ProjectCaptureSnapshot {
    capture_snapshot(cwd, git_program).unwrap_or_else(|_| unresolved_snapshot(cwd))
}

fn capture_wsl_snapshot_or_unresolved(
    context: WslCaptureContext,
    wsl_executable: &Path,
) -> ProjectCaptureSnapshot {
    let fallback = context.unresolved_snapshot();
    capture_wsl_snapshot(&context, wsl_executable).unwrap_or(fallback)
}

fn capture_wsl_snapshot(
    context: &WslCaptureContext,
    wsl_executable: &Path,
) -> Result<ProjectCaptureSnapshot, IdentityError> {
    let command = wsl_origin_command(wsl_executable, context);
    let output = run_command_bounded(command, WSL_GIT_COMMAND_TIMEOUT)?;
    if !output.status.success() {
        return Err(IdentityError::GitCommandFailed(
            String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        ));
    }
    let origin = parse_wsl_origin(context, &output.stdout)?;
    Ok(ProjectCaptureSnapshot {
        submitted_cwd: PathBuf::from(&context.cwd),
        origin,
    })
}

fn wsl_origin_command(wsl_executable: &Path, context: &WslCaptureContext) -> Command {
    let mut command = Command::new(wsl_executable);
    command.args([
        "-d",
        &context.distro,
        "--",
        "sh",
        "-c",
        WSL_ORIGIN_SCRIPT,
        "akra-wsl-origin",
        &context.cwd,
    ]);
    command
}

fn parse_wsl_origin(
    context: &WslCaptureContext,
    output: &[u8],
) -> Result<ProjectOriginSnapshot, IdentityError> {
    if output.len() > MAX_WSL_ORIGIN_OUTPUT_BYTES {
        return Err(IdentityError::OversizedWslOutput(output.len()));
    }
    let fields = output.split(|byte| *byte == 0).collect::<Vec<_>>();
    match fields.as_slice() {
        [b"git", common_dir, display_path, b""] => {
            let common_dir = parse_wsl_path(common_dir)?;
            let display_path = parse_wsl_path(display_path)?;
            Ok(ProjectOriginSnapshot {
                identity: wsl_project_key(&context.identity_distro, &common_dir),
                kind: ProjectOriginKind::Git,
                display_path: wsl_display_path(&context.distro, &display_path),
            })
        }
        [b"directory", directory, b""] => {
            let directory = parse_wsl_path(directory)?;
            Ok(ProjectOriginSnapshot {
                identity: wsl_project_key(&context.identity_distro, &directory),
                kind: ProjectOriginKind::Directory,
                display_path: wsl_display_path(&context.distro, &directory),
            })
        }
        _ => Err(IdentityError::InvalidWslOutput),
    }
}

fn parse_wsl_path(value: &[u8]) -> Result<String, IdentityError> {
    let value = std::str::from_utf8(value).map_err(IdentityError::WslOutputUtf8)?;
    validate_linux_absolute_path(value)?;
    Ok(value.to_owned())
}

fn validate_wsl_distro(distro: &str) -> Result<(), IdentityError> {
    let invalid_component = matches!(distro, "." | "..")
        || distro.is_empty()
        || distro.len() > MAX_WSL_DISTRO_BYTES
        || !distro.chars().next().is_some_and(char::is_alphanumeric)
        || !distro.chars().last().is_some_and(char::is_alphanumeric)
        || distro.chars().any(|character| {
            character.is_control()
                || matches!(
                    character,
                    '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
                )
        });
    if invalid_component {
        return Err(IdentityError::InvalidWslDistro(distro.to_owned()));
    }
    Ok(())
}

fn validate_linux_absolute_path(path: &str) -> Result<(), IdentityError> {
    if !path.starts_with('/')
        || path.len() > MAX_WSL_PATH_BYTES
        || path
            .chars()
            .any(|character| character.is_control() || character == '\\')
    {
        return Err(IdentityError::InvalidWslPath(path.to_owned()));
    }
    Ok(())
}

fn wsl_project_key(identity_distro: &str, linux_path: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(b"wsl\0");
    digest.update(identity_distro.as_bytes());
    digest.update(b"\0");
    digest.update(linux_path.as_bytes());
    hex::encode(digest.finalize())
}

fn wsl_display_path(distro: &str, linux_path: &str) -> PathBuf {
    PathBuf::from(format!("wsl://{distro}{linux_path}"))
}

#[cfg(windows)]
fn windows_wsl_executable() -> Option<PathBuf> {
    let system_root = std::env::var_os("SystemRoot")?;
    let executable = PathBuf::from(system_root).join("System32").join("wsl.exe");
    (executable.is_absolute()
        && executable
            .metadata()
            .is_ok_and(|metadata| metadata.is_file()))
    .then_some(executable)
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
    let git_paths = if git_environment_requires_process() {
        git_project_paths(&worktree_path, git_program)?
    } else {
        match filesystem_git_project_paths(&worktree_path) {
            Ok(paths) => paths,
            Err(_) => git_project_paths(&worktree_path, git_program)?,
        }
    };
    match git_paths {
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

fn git_environment_requires_process() -> bool {
    [
        "GIT_DIR",
        "GIT_WORK_TREE",
        "GIT_COMMON_DIR",
        "GIT_CEILING_DIRECTORIES",
        "GIT_DISCOVERY_ACROSS_FILESYSTEM",
    ]
    .into_iter()
    .any(|name| std::env::var_os(name).is_some())
}

fn filesystem_git_project_paths(cwd: &Path) -> io::Result<Option<(PathBuf, PathBuf)>> {
    for root in cwd.ancestors() {
        let marker = root.join(".git");
        let metadata = match fs::symlink_metadata(&marker) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == ErrorKind::NotFound => {
                if root.join("HEAD").is_file()
                    && root.join("objects").is_dir()
                    && root.join("refs").is_dir()
                {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "bare repository requires Git discovery",
                    ));
                }
                continue;
            }
            Err(error) => return Err(error),
        };
        let git_dir = if metadata.file_type().is_dir() {
            marker.canonicalize()?
        } else if metadata.file_type().is_file() {
            resolve_git_pointer(root, &marker)?
        } else {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "unsupported .git marker",
            ));
        };
        let common_dir = resolve_common_dir(&git_dir)?;
        if !git_dir.join("HEAD").is_file()
            || !common_dir.join("objects").is_dir()
            || !common_dir.join("refs").is_dir()
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "incomplete Git directory",
            ));
        }
        return Ok(Some((common_dir, root.to_path_buf())));
    }
    Ok(None)
}

fn resolve_git_pointer(worktree_root: &Path, marker: &Path) -> io::Result<PathBuf> {
    let pointer = read_bounded_git_text(marker)?;
    let mut lines = pointer.lines();
    let line = lines
        .next()
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "empty .git pointer"))?;
    if lines.next().is_some() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "multiline .git pointer",
        ));
    }
    let value = line
        .strip_prefix("gitdir:")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| io::Error::new(ErrorKind::InvalidData, "invalid .git pointer"))?;
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        worktree_root.join(path)
    };
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            ".git pointer is not a directory",
        ));
    }
    Ok(path)
}

fn resolve_common_dir(git_dir: &Path) -> io::Result<PathBuf> {
    let marker = git_dir.join("commondir");
    let value = match read_bounded_git_text(&marker) {
        Ok(value) => value,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(git_dir.to_path_buf()),
        Err(error) => return Err(error),
    };
    let value = value.trim();
    if value.is_empty() || value.lines().count() != 1 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "invalid Git commondir",
        ));
    }
    let path = PathBuf::from(value);
    let path = if path.is_absolute() {
        path
    } else {
        git_dir.join(path)
    };
    let path = path.canonicalize()?;
    if !path.is_dir() {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Git commondir is not a directory",
        ));
    }
    Ok(path)
}

fn read_bounded_git_text(path: &Path) -> io::Result<String> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.file_type().is_file() || metadata.len() > MAX_GIT_POINTER_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Git pointer is not a bounded regular file",
        ));
    }
    let mut value = String::new();
    File::open(path)?
        .take(MAX_GIT_POINTER_BYTES + 1)
        .read_to_string(&mut value)?;
    if value.len() as u64 > MAX_GIT_POINTER_BYTES {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Git pointer exceeds size limit",
        ));
    }
    Ok(value)
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
    #[error("invalid WSL distribution name: {0}")]
    InvalidWslDistro(String),
    #[error("WSL working directory must be a bounded absolute Linux path: {0}")]
    InvalidWslPath(String),
    #[error("WSL Git identity command returned invalid UTF-8: {0}")]
    WslOutputUtf8(#[source] std::str::Utf8Error),
    #[error("WSL Git identity command returned an invalid result")]
    InvalidWslOutput,
    #[error("WSL Git identity command returned {0} bytes, exceeding the output limit")]
    OversizedWslOutput(usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn directory_capture_does_not_require_a_git_process() {
        let directory = TempDir::new().expect("working directory");
        let root = directory
            .path()
            .ancestors()
            .last()
            .expect("filesystem root");
        let snapshot = capture_snapshot_or_unresolved(root, &directory.path().join("missing-git"));

        assert_eq!(snapshot.origin.kind, ProjectOriginKind::Directory);
        assert_eq!(snapshot.origin.display_path, root);
    }

    #[test]
    fn standard_git_capture_does_not_require_a_git_process() {
        let directory = TempDir::new().expect("working directory");
        let git_dir = directory.path().join(".git");
        fs::create_dir(&git_dir).expect("Git directory");
        fs::write(git_dir.join("HEAD"), "ref: refs/heads/main\n").expect("HEAD");
        fs::create_dir(git_dir.join("objects")).expect("objects");
        fs::create_dir(git_dir.join("refs")).expect("refs");

        let snapshot =
            capture_snapshot_or_unresolved(directory.path(), &directory.path().join("missing-git"));

        assert_eq!(snapshot.origin.kind, ProjectOriginKind::Git);
        assert_eq!(snapshot.origin.display_path, directory.path());
        assert_eq!(
            snapshot.origin.identity,
            project_key(&directory.path().join(".git").canonicalize().expect(".git"))
        );
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
    fn wsl_context_validates_namespace_and_absolute_linux_path() {
        assert!(WslCaptureContext::new("Ubuntu-24.04", "/home/alex/project").is_ok());
        assert!(WslCaptureContext::new("Ubuntu Dev", "/home/alex/project").is_ok());
        assert!(matches!(
            WslCaptureContext::new("../Ubuntu", "/home/alex/project"),
            Err(IdentityError::InvalidWslDistro(_))
        ));
        assert!(matches!(
            WslCaptureContext::new("Ubuntu", "home/alex/project"),
            Err(IdentityError::InvalidWslPath(_))
        ));
        assert!(matches!(
            WslCaptureContext::new("Ubuntu", "/home/alex/project\nnext"),
            Err(IdentityError::InvalidWslPath(_))
        ));
    }

    #[test]
    fn wsl_command_passes_untrusted_path_text_as_one_quoted_shell_argument() {
        let cwd = "/tmp/project;printf injected";
        let context = WslCaptureContext::new("Ubuntu Dev", cwd).expect("valid context");
        let command = wsl_origin_command(Path::new("wsl.exe"), &context);
        let arguments = command
            .get_args()
            .map(|argument| argument.to_string_lossy().into_owned())
            .collect::<Vec<_>>();

        assert_eq!(arguments[0..3], ["-d", "Ubuntu Dev", "--"]);
        assert_eq!(arguments[3], "sh");
        assert_eq!(arguments[4], "-c");
        assert_eq!(arguments[5], WSL_ORIGIN_SCRIPT);
        assert_eq!(arguments[6], "akra-wsl-origin");
        assert_eq!(arguments[7], cwd);
        assert!(WSL_ORIGIN_SCRIPT.contains("git -C \"$cwd\""));
    }

    #[test]
    fn wsl_git_origins_share_worktree_identity_and_include_distro_namespace() {
        let ubuntu = WslCaptureContext::new("Ubuntu", "/home/alex/main").expect("Ubuntu");
        let other = WslCaptureContext::new("Debian", "/home/alex/main").expect("Debian");
        let main = parse_wsl_origin(&ubuntu, b"git\0/home/alex/main/.git\0/home/alex/main\0")
            .expect("main origin");
        let linked = parse_wsl_origin(&ubuntu, b"git\0/home/alex/main/.git\0/home/alex/linked\0")
            .expect("linked origin");
        let other_distro =
            parse_wsl_origin(&other, b"git\0/home/alex/main/.git\0/home/alex/main\0")
                .expect("other distro origin");

        assert_eq!(main.kind, ProjectOriginKind::Git);
        assert_eq!(main.identity, linked.identity);
        assert_ne!(main.identity, other_distro.identity);
        assert_eq!(main.display_path, Path::new("wsl://Ubuntu/home/alex/main"));
        assert_eq!(
            linked.display_path,
            Path::new("wsl://Ubuntu/home/alex/linked")
        );
        assert_eq!(
            other_distro.display_path,
            Path::new("wsl://Debian/home/alex/main")
        );
    }

    #[test]
    fn wsl_directory_and_unresolved_origins_are_stable_and_namespaced() {
        let context = WslCaptureContext::new("Ubuntu", "/home/alex/plain").expect("context");
        let directory =
            parse_wsl_origin(&context, b"directory\0/home/alex/plain\0").expect("directory origin");
        let first = context.unresolved_snapshot();
        let second = context.unresolved_snapshot();

        assert_eq!(directory.kind, ProjectOriginKind::Directory);
        assert_eq!(
            directory.display_path,
            Path::new("wsl://Ubuntu/home/alex/plain")
        );
        assert_eq!(first, second);
        assert!(first.origin.identity.starts_with("unresolved:"));
        assert_eq!(
            first.origin.display_path,
            Path::new("wsl://Ubuntu/home/alex/plain")
        );
    }

    #[test]
    fn unavailable_wsl_launcher_falls_back_without_losing_namespace() {
        let directory = TempDir::new().expect("working directory");
        let context = WslCaptureContext::new("Ubuntu", "/home/alex/project").expect("context");
        let snapshot =
            capture_wsl_snapshot_or_unresolved(context, &directory.path().join("missing-wsl"));

        assert_eq!(snapshot.origin.kind, ProjectOriginKind::Unresolved);
        assert_eq!(
            snapshot.origin.display_path,
            Path::new("wsl://Ubuntu/home/alex/project")
        );
    }

    #[test]
    fn wsl_protocol_rejects_relative_or_oversized_output() {
        let context = WslCaptureContext::new("Ubuntu", "/home/alex/project").expect("context");
        assert!(matches!(
            parse_wsl_origin(&context, b"git\0relative/.git\0/home/alex/project\0"),
            Err(IdentityError::InvalidWslPath(_))
        ));
        assert!(matches!(
            parse_wsl_origin(&context, &vec![b'x'; MAX_WSL_ORIGIN_OUTPUT_BYTES + 1]),
            Err(IdentityError::OversizedWslOutput(_))
        ));
    }
}
