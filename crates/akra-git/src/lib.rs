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
    display_path: PathBuf,
    worktree_path: PathBuf,
}

impl ProjectIdentity {
    pub fn from_cwd(cwd: &Path) -> Result<Self, IdentityError> {
        let worktree_path = cwd.canonicalize().map_err(IdentityError::Canonicalize)?;
        let (key_path, display_path) = match git_common_dir(&worktree_path) {
            Some(common_dir) => (
                common_dir.clone(),
                common_dir
                    .parent()
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| worktree_path.clone()),
            ),
            None => (worktree_path.clone(), worktree_path.clone()),
        };
        let key = hex::encode(Sha256::digest(key_path.to_string_lossy().as_bytes()));
        Ok(Self {
            key,
            display_path,
            worktree_path,
        })
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
