//! Crash-safe ingress spooling.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicUsize},
};

use akra_git::ProjectOriginSnapshot;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

#[path = "spool/queue.rs"]
mod queue;

pub const CAPTURE_ENVELOPE_SCHEMA_VERSION: u8 = 1;
pub const MAX_CAPTURE_INPUT_BYTES: usize = 256 * 1024;
pub const MAX_PENDING_ITEM_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PENDING_ITEMS: usize = 1024;
pub const MAX_PENDING_BYTES: u64 = 64 * 1024 * 1024;
pub const RECOVERY_BATCH_SIZE: usize = 32;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureEnvelope {
    schema_version: u8,
    provider: String,
    captured_at_us: i64,
    origin: ProjectOriginSnapshot,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    capture_source: Option<CaptureSource>,
    payload: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSource {
    target: String,
    client: String,
}

impl CaptureEnvelope {
    pub fn new(
        provider: &str,
        captured_at_us: i64,
        origin: ProjectOriginSnapshot,
        payload: serde_json::Value,
    ) -> Result<Self, CaptureEnvelopeError> {
        let envelope = Self {
            schema_version: CAPTURE_ENVELOPE_SCHEMA_VERSION,
            provider: provider.to_owned(),
            captured_at_us,
            origin,
            capture_source: None,
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn new_with_source(
        provider: &str,
        captured_at_us: i64,
        origin: ProjectOriginSnapshot,
        payload: serde_json::Value,
        target: &str,
        client: &str,
    ) -> Result<Self, CaptureEnvelopeError> {
        let envelope = Self {
            schema_version: CAPTURE_ENVELOPE_SCHEMA_VERSION,
            provider: provider.to_owned(),
            captured_at_us,
            origin,
            capture_source: Some(CaptureSource {
                target: target.to_owned(),
                client: client.to_owned(),
            }),
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn decode(payload: &[u8]) -> Result<Self, CaptureEnvelopeError> {
        let envelope: Self = serde_json::from_slice(payload)?;
        if envelope.schema_version != CAPTURE_ENVELOPE_SCHEMA_VERSION {
            return Err(CaptureEnvelopeError::UnsupportedSchemaVersion(
                envelope.schema_version,
            ));
        }
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn provider(&self) -> &str {
        &self.provider
    }

    pub const fn captured_at_us(&self) -> i64 {
        self.captured_at_us
    }

    pub fn origin(&self) -> &ProjectOriginSnapshot {
        &self.origin
    }

    pub fn capture_source(&self) -> Option<(&str, &str)> {
        self.capture_source
            .as_ref()
            .map(|source| (source.target.as_str(), source.client.as_str()))
    }

    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    fn validate(&self) -> Result<(), CaptureEnvelopeError> {
        if self.provider.trim().is_empty() {
            return Err(CaptureEnvelopeError::BlankProvider);
        }
        if self.captured_at_us < 0 {
            return Err(CaptureEnvelopeError::NegativeCaptureTime(
                self.captured_at_us,
            ));
        }
        if self.origin.identity.trim().is_empty() {
            return Err(CaptureEnvelopeError::BlankOriginIdentity);
        }
        if let Some(source) = &self.capture_source
            && (!valid_source_token(&source.target, 128) || !valid_source_token(&source.client, 32))
        {
            return Err(CaptureEnvelopeError::InvalidCaptureSource);
        }
        Ok(())
    }
}

fn valid_source_token(value: &str, max_len: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_len
        && value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '_' | '-' | ':')
        })
}

#[derive(Debug)]
pub struct Spool {
    directory: PathBuf,
    deferred: Mutex<HashSet<PathBuf>>,
    recovery_offset: AtomicUsize,
}

#[derive(Debug)]
pub struct SpoolItem {
    path: PathBuf,
}

impl Spool {
    pub fn open(directory: &Path) -> Result<Self, SpoolError> {
        fs::create_dir_all(directory)?;
        Ok(Self {
            directory: directory.to_path_buf(),
            deferred: Mutex::new(HashSet::new()),
            recovery_offset: AtomicUsize::new(0),
        })
    }

    pub fn enqueue(&self, payload: &[u8]) -> Result<(), SpoolError> {
        if payload.len() > MAX_PENDING_ITEM_BYTES {
            return Err(SpoolError::Oversized(payload.len() as u64));
        }
        let _admission = self.lock_admission()?;
        self.ensure_capacity(payload.len() as u64)?;
        let key = Uuid::new_v4();
        let pending = self.directory.join(format!("{key}.pending"));
        let temporary = self.directory.join(format!("{key}.tmp"));
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(payload)?;
        file.sync_all()?;
        fs::rename(temporary, pending)?;
        sync_directory(&self.directory)?;
        Ok(())
    }

    pub fn enqueue_envelope(&self, envelope: &CaptureEnvelope) -> Result<(), SpoolError> {
        self.enqueue(&serde_json::to_vec(envelope)?)
    }
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
        options.custom_flags(FILE_FLAG_BACKUP_SEMANTICS);
    }
    let result = options
        .open(path)
        .and_then(|directory| directory.sync_all());
    #[cfg(windows)]
    if result
        .as_ref()
        .is_err_and(|error| error.kind() == std::io::ErrorKind::PermissionDenied)
    {
        return Ok(());
    }
    result
}

#[derive(Debug, Error)]
pub enum SpoolError {
    #[error("filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
    #[error("capture envelope serialization failed: {0}")]
    Json(#[from] serde_json::Error),
    #[error("pending spool item is not a regular file")]
    NonRegular,
    #[error("pending spool item is {0} bytes, exceeding the {MAX_PENDING_ITEM_BYTES}-byte limit")]
    Oversized(u64),
    #[error(
        "pending spool queue is full ({items} items, {bytes} bytes; limits are {MAX_PENDING_ITEMS} items and {MAX_PENDING_BYTES} bytes)"
    )]
    QueueFull { items: usize, bytes: u64 },
    #[error("spool admission stopped after inspecting {entries} directory entries")]
    QueueInspectionLimit { entries: usize },
    #[error("spool queue state is unavailable")]
    StatePoisoned,
}

#[derive(Debug, Error)]
pub enum CaptureEnvelopeError {
    #[error("capture provider must not be blank")]
    BlankProvider,
    #[error("capture timestamp must not be negative: {0}")]
    NegativeCaptureTime(i64),
    #[error("capture origin identity must not be blank")]
    BlankOriginIdentity,
    #[error("capture source target or client identifier is invalid")]
    InvalidCaptureSource,
    #[error("unsupported capture envelope schema version: {0}")]
    UnsupportedSchemaVersion(u8),
    #[error("invalid capture envelope JSON: {0}")]
    Json(#[from] serde_json::Error),
}
