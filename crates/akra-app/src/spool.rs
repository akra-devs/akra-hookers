//! Crash-safe ingress spooling.

use std::{
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use thiserror::Error;

#[derive(Debug)]
pub struct Spool {
    directory: PathBuf,
}

impl Spool {
    pub fn open(directory: &Path) -> Result<Self, SpoolError> {
        fs::create_dir_all(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
        })
    }

    pub fn enqueue(&self, payload: &[u8]) -> Result<(), SpoolError> {
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| SpoolError::ClockBeforeEpoch)?
            .as_nanos();
        let pending = self.directory.join(format!("{stamp}.pending"));
        let temporary = self.directory.join(format!("{stamp}.tmp"));
        fs::write(&temporary, payload)?;
        fs::rename(temporary, pending)?;
        Ok(())
    }

    pub fn drain(&self) -> Result<Vec<Vec<u8>>, SpoolError> {
        let mut paths = fs::read_dir(&self.directory)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension()
                    .is_some_and(|extension| extension == "pending")
            })
            .collect::<Vec<_>>();
        paths.sort();

        let mut payloads = Vec::with_capacity(paths.len());
        for path in paths {
            payloads.push(fs::read(&path)?);
            fs::remove_file(path)?;
        }
        Ok(payloads)
    }
}

#[derive(Debug, Error)]
pub enum SpoolError {
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("system clock is before Unix epoch")]
    ClockBeforeEpoch,
}
