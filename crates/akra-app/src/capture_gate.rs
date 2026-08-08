use std::{
    fs,
    io::{self, ErrorKind},
    path::{Path, PathBuf},
};

use thiserror::Error;

const GATE_FILE: &str = "capture-enabled";

#[derive(Clone, Debug)]
pub struct CaptureGate {
    path: PathBuf,
}

impl CaptureGate {
    pub fn new(data_dir: &Path) -> Self {
        Self {
            path: data_dir.join(GATE_FILE),
        }
    }

    pub fn is_enabled(&self) -> Result<bool, CaptureGateError> {
        match fs::read_to_string(&self.path) {
            Ok(value) => match value.trim() {
                "true" => Ok(true),
                "false" => Ok(false),
                value => Err(CaptureGateError::InvalidState(value.to_owned())),
            },
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(true),
            Err(error) => Err(CaptureGateError::Io(error)),
        }
    }

    pub fn set_enabled(&self, enabled: bool) -> Result<(), CaptureGateError> {
        let parent = self.path.parent().ok_or(CaptureGateError::MissingParent)?;
        fs::create_dir_all(parent)?;
        fs::write(&self.path, if enabled { "true" } else { "false" })?;
        Ok(())
    }
}

#[derive(Debug, Error)]
pub enum CaptureGateError {
    #[error("capture gate has no parent directory")]
    MissingParent,
    #[error("capture gate filesystem operation failed: {0}")]
    Io(#[from] io::Error),
    #[error("capture gate state is invalid: {0}")]
    InvalidState(String),
}
