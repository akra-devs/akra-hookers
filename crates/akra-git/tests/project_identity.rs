use std::{path::Path, process::Command};

use akra_git::ProjectIdentity;
use tempfile::TempDir;

fn git(directory: &Path, arguments: &[&str]) {
    let status = Command::new("git")
        .args(arguments)
        .current_dir(directory)
        .status()
        .expect("git launches");
    assert!(status.success(), "git command failed: {arguments:?}");
}

#[test]
fn linked_worktrees_share_project_identity() {
    let root = TempDir::new().expect("temp root");
    let main = root.path().join("main");
    std::fs::create_dir(&main).expect("main directory");
    git(&main, &["init"]);
    git(&main, &["config", "user.email", "test@example.invalid"]);
    git(&main, &["config", "user.name", "Test"]);
    std::fs::write(main.join("README.md"), "fixture").expect("fixture");
    git(&main, &["add", "."]);
    git(&main, &["commit", "-m", "initial"]);
    let linked = root.path().join("linked");
    git(&main, &["worktree", "add", linked.to_str().expect("utf-8")]);

    let main_identity = ProjectIdentity::from_cwd(&main).expect("main identity");
    let linked_identity = ProjectIdentity::from_cwd(&linked).expect("linked identity");

    assert_eq!(main_identity.key(), linked_identity.key());
    assert_ne!(
        main_identity.worktree_path(),
        linked_identity.worktree_path()
    );
}

#[test]
fn non_git_directory_displays_itself_and_not_its_parent() {
    let root = TempDir::new().expect("temp root");
    let client_a = root.path().join("client-a");
    let client_b = root.path().join("client-b");
    std::fs::create_dir(&client_a).expect("client A directory");
    std::fs::create_dir(&client_b).expect("client B directory");

    let client_a_identity = ProjectIdentity::from_cwd(&client_a).expect("client A identity");
    let client_b_identity = ProjectIdentity::from_cwd(&client_b).expect("client B identity");

    assert_eq!(
        client_a_identity.display_path(),
        client_a.canonicalize().expect("canonical client A")
    );
    assert_ne!(client_a_identity.key(), client_b_identity.key());
}
