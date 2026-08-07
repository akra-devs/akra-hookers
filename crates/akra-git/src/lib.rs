#![forbid(unsafe_code)]

use std::{
    path::{Path, PathBuf},
    process::Command,
};

use sha2::{Digest, Sha256};
use thiserror::Error;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProjectIdentity {
    key: String,
    worktree_path: PathBuf,
}

impl ProjectIdentity {
    pub fn from_cwd(cwd: &Path) -> Result<Self, IdentityError> {
        let worktree_path = cwd.canonicalize().map_err(IdentityError::Canonicalize)?;
        let common_dir = git_common_dir(&worktree_path).unwrap_or_else(|| worktree_path.clone());
        let key = hex::encode(Sha256::digest(common_dir.to_string_lossy().as_bytes()));
        Ok(Self { key, worktree_path })
    }

    pub fn key(&self) -> &str {
        &self.key
    }

    pub fn worktree_path(&self) -> &Path {
        &self.worktree_path
    }
}

fn git_common_dir(cwd: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    PathBuf::from(path.trim()).canonicalize().ok()
}

#[derive(Debug, Error)]
pub enum IdentityError {
    #[error("failed to canonicalize working directory: {0}")]
    Canonicalize(#[source] std::io::Error),
}
