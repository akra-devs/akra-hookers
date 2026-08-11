use akra_git::{ProjectIdentity, ProjectOriginKind};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

static NEXT_TEMP_DIR: AtomicUsize = AtomicUsize::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "akra-git-project-identity-{label}-{}-{}",
            std::process::id(),
            NEXT_TEMP_DIR.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn git(cwd: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn repo(parent: &Path, name: &str) -> PathBuf {
    let path = parent.join(name);
    fs::create_dir_all(&path).unwrap();
    git(&path, &["init"]);
    git(&path, &["config", "user.email", "tests@example.invalid"]);
    git(&path, &["config", "user.name", "akra-git tests"]);
    fs::write(path.join("README"), "initial\n").unwrap();
    git(&path, &["add", "README"]);
    git(&path, &["commit", "-m", "initial"]);
    path
}

#[test]
fn capture_snapshot_uses_one_git_origin_for_linked_worktrees_but_keeps_submitted_cwds() {
    let temp = TempDir::new("worktrees");
    let primary = repo(temp.path(), "primary");
    let linked = temp.path().join("linked");
    git(
        &primary,
        &[
            "worktree",
            "add",
            "-b",
            "linked-branch",
            linked.to_str().unwrap(),
        ],
    );

    let primary_cwd = primary.join("nested-primary");
    let linked_cwd = linked.join("nested-linked");
    fs::create_dir_all(&primary_cwd).unwrap();
    fs::create_dir_all(&linked_cwd).unwrap();

    let primary_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&primary_cwd).unwrap();
    let linked_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&linked_cwd).unwrap();

    assert_eq!(primary_snapshot.origin.kind, ProjectOriginKind::Git);
    assert_eq!(
        primary_snapshot.origin.identity,
        linked_snapshot.origin.identity
    );
    assert_ne!(
        primary_snapshot.submitted_cwd,
        linked_snapshot.submitted_cwd
    );
    assert_ne!(
        primary_snapshot.submitted_cwd,
        primary_snapshot.origin.display_path
    );
    assert_ne!(
        linked_snapshot.submitted_cwd,
        linked_snapshot.origin.display_path
    );
    assert!(ProjectIdentity::from_cwd(&primary_cwd).is_ok());
}

#[test]
fn capture_snapshot_gives_separate_clones_distinct_git_origins() {
    let temp = TempDir::new("clones");
    let source = repo(temp.path(), "source");
    let clone = temp.path().join("clone");
    git(
        temp.path(),
        &["clone", source.to_str().unwrap(), clone.to_str().unwrap()],
    );

    let source_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&source).unwrap();
    let clone_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&clone).unwrap();

    assert_eq!(source_snapshot.origin.kind, ProjectOriginKind::Git);
    assert_eq!(clone_snapshot.origin.kind, ProjectOriginKind::Git);
    assert_ne!(
        source_snapshot.origin.identity,
        clone_snapshot.origin.identity
    );
}

#[test]
fn capture_snapshot_gives_existing_non_git_directories_distinct_directory_origins() {
    let temp = TempDir::new("directories");
    let first = temp.path().join("first");
    let second = temp.path().join("second");
    fs::create_dir_all(&first).unwrap();
    fs::create_dir_all(&second).unwrap();

    let first_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&first).unwrap();
    let second_snapshot = ProjectIdentity::capture_snapshot_from_cwd(&second).unwrap();

    assert_eq!(first_snapshot.origin.kind, ProjectOriginKind::Directory);
    assert_eq!(second_snapshot.origin.kind, ProjectOriginKind::Directory);
    assert_ne!(
        first_snapshot.origin.identity,
        second_snapshot.origin.identity
    );
}

#[test]
fn capture_snapshot_uses_a_stable_unresolved_origin_for_a_deleted_cwd() {
    let temp = TempDir::new("unresolved");
    let missing = temp.path().join("deleted");
    fs::create_dir_all(&missing).unwrap();
    fs::remove_dir(&missing).unwrap();

    let first = ProjectIdentity::capture_snapshot_from_cwd(&missing).unwrap();
    let second = ProjectIdentity::capture_snapshot_from_cwd(&missing).unwrap();

    assert_eq!(first.origin.kind, ProjectOriginKind::Unresolved);
    assert_eq!(first.origin.identity, second.origin.identity);
    assert_eq!(first.origin.display_path, second.origin.display_path);
    assert_ne!(first.origin.identity, missing.to_string_lossy());
}

#[test]
fn capture_snapshot_uses_the_git_root_as_display_fallback_and_serializes_origin_fields() {
    let temp = TempDir::new("serde");
    let root = repo(temp.path(), "root");
    let snapshot = ProjectIdentity::capture_snapshot_from_cwd(&root).unwrap();

    assert_eq!(snapshot.origin.kind, ProjectOriginKind::Git);
    assert_eq!(snapshot.origin.display_path, root);

    let value = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(value["origin"]["kind"], "git");
    assert!(value["origin"].get("identity").is_some());
    assert_eq!(
        value["origin"]["display_path"],
        root.to_string_lossy().as_ref()
    );
    let decoded: akra_git::ProjectCaptureSnapshot = serde_json::from_value(value).unwrap();
    assert_eq!(decoded, snapshot);
}
