//! Crash-safe ingress spooling.

use std::{
    fs,
    path::{Path, PathBuf},
};

use thiserror::Error;
use uuid::Uuid;

#[derive(Debug)]
pub struct Spool {
    directory: PathBuf,
}

#[derive(Debug)]
pub struct SpoolItem {
    path: PathBuf,
    payload: Vec<u8>,
}

impl SpoolItem {
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

impl Spool {
    pub fn open(directory: &Path) -> Result<Self, SpoolError> {
        fs::create_dir_all(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
        })
    }

    pub fn enqueue(&self, payload: &[u8]) -> Result<(), SpoolError> {
        let key = Uuid::new_v4();
        let pending = self.directory.join(format!("{key}.pending"));
        let temporary = self.directory.join(format!("{key}.tmp"));
        fs::write(&temporary, payload)?;
        fs::rename(temporary, pending)?;
        Ok(())
    }

    pub fn drain(&self) -> Result<Vec<Vec<u8>>, SpoolError> {
        Ok(self
            .pending()?
            .into_iter()
            .map(|item| item.payload)
            .collect())
    }

    pub fn pending(&self) -> Result<Vec<SpoolItem>, SpoolError> {
        let mut paths = fs::read_dir(&self.directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "pending")
            })
            .collect::<Vec<_>>();
        paths.sort();

        let mut items = Vec::with_capacity(paths.len());
        for path in paths {
            items.push(SpoolItem {
                payload: fs::read(&path)?,
                path,
            });
        }
        Ok(items)
    }

    pub fn acknowledge(&self, item: SpoolItem) -> Result<(), SpoolError> {
        fs::remove_file(item.path)?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum SpoolError {
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
