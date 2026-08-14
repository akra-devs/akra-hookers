//! Crash-safe ingress spooling.

use std::{
    collections::HashSet,
    fs::{self, OpenOptions},
    io::Write,
    path::{Path, PathBuf},
    sync::{Mutex, atomic::AtomicUsize},
};

use akra_core::ingress::ActivityKind;
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
    #[serde(default, skip_serializing_if = "CaptureActivity::is_user")]
    activity: CaptureActivity,
    payload: serde_json::Value,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureSource {
    target: String,
    client: String,
}

#[derive(Clone, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct CaptureActivity {
    kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_type: Option<String>,
}

impl CaptureActivity {
    fn is_user(&self) -> bool {
        self.kind == ActivityKind::User && self.agent_id.is_none() && self.agent_type.is_none()
    }
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
            activity: CaptureActivity::default(),
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
            activity: CaptureActivity::default(),
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    pub fn new_with_activity(
        provider: &str,
        captured_at_us: i64,
        origin: ProjectOriginSnapshot,
        payload: serde_json::Value,
        kind: ActivityKind,
        agent_id: Option<String>,
        agent_type: Option<String>,
    ) -> Result<Self, CaptureEnvelopeError> {
        let envelope = Self {
            schema_version: CAPTURE_ENVELOPE_SCHEMA_VERSION,
            provider: provider.to_owned(),
            captured_at_us,
            origin,
            capture_source: None,
            activity: CaptureActivity {
                kind,
                agent_id,
                agent_type,
            },
            payload,
        };
        envelope.validate()?;
        Ok(envelope)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn new_with_source_and_activity(
        provider: &str,
        captured_at_us: i64,
        origin: ProjectOriginSnapshot,
        payload: serde_json::Value,
        target: &str,
        client: &str,
        kind: ActivityKind,
        agent_id: Option<String>,
        agent_type: Option<String>,
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
            activity: CaptureActivity {
                kind,
                agent_id,
                agent_type,
            },
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

    pub fn activity_context(&self) -> (ActivityKind, Option<&str>, Option<&str>) {
        (
            self.activity.kind,
            self.activity.agent_id.as_deref(),
            self.activity.agent_type.as_deref(),
        )
    }

    pub fn payload(&self) -> &serde_json::Value {
        &self.payload
    }

    /// Namespaces an envelope received from another machine and removes the
    /// sender's local runtime hint. The collector must never try to execute a
    /// summary with a capture target that only exists on the source machine.
    pub fn into_remote_namespace(
        mut self,
        source_instance_id: &str,
    ) -> Result<Self, CaptureEnvelopeError> {
        if !valid_source_token(source_instance_id, 64) {
            return Err(CaptureEnvelopeError::InvalidRemoteSource);
        }
        let payload = self
            .payload
            .as_object_mut()
            .ok_or(CaptureEnvelopeError::InvalidRemotePayload)?;
        let session_id = payload
            .get_mut("session_id")
            .and_then(|value| value.as_str())
            .ok_or(CaptureEnvelopeError::InvalidRemotePayload)?
            .to_owned();
        payload.insert(
            "session_id".to_owned(),
            serde_json::Value::String(format!("remote:{source_instance_id}:{session_id}")),
        );
        self.origin.identity = format!("remote:{source_instance_id}:{}", self.origin.identity);
        self.capture_source = None;
        self.validate()?;
        Ok(self)
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
        if self.activity.kind != ActivityKind::Subagent
            && (self.activity.agent_id.is_some() || self.activity.agent_type.is_some())
        {
            return Err(CaptureEnvelopeError::InvalidActivityContext);
        }
        for value in [
            self.activity.agent_id.as_deref(),
            self.activity.agent_type.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            if value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control) {
                return Err(CaptureEnvelopeError::InvalidActivityContext);
            }
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

impl SpoolItem {
    /// Returns the opaque pending-file path to crate-local queue extensions.
    /// Callers must not derive capture content or a destination from the name.
    pub(crate) fn path(&self) -> &Path {
        &self.path
    }
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
        self.enqueue_with_kind(payload, false)
    }

    /// Enqueues an opaque payload while retaining the result marker used by
    /// bounded-retention queues such as the remote collector outbox.
    pub fn enqueue_marked(&self, payload: &[u8], result_capture: bool) -> Result<(), SpoolError> {
        self.enqueue_with_kind(payload, result_capture)
    }

    fn enqueue_with_kind(&self, payload: &[u8], result_capture: bool) -> Result<(), SpoolError> {
        if payload.len() > MAX_PENDING_ITEM_BYTES {
            return Err(SpoolError::Oversized(payload.len() as u64));
        }
        let _admission = self.lock_admission()?;
        self.ensure_capacity(payload.len() as u64)?;
        let key = Uuid::new_v4();
        let kind = if result_capture { ".result" } else { "" };
        let pending = self.directory.join(format!("{key}{kind}.pending"));
        let temporary = self.directory.join(format!("{key}{kind}.tmp"));
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
        self.enqueue_with_kind(
            &serde_json::to_vec(envelope)?,
            envelope
                .payload
                .get("hook_event_name")
                .and_then(serde_json::Value::as_str)
                == Some("Stop"),
        )
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
    #[error("capture activity context is invalid")]
    InvalidActivityContext,
    #[error("remote source instance identifier is invalid")]
    InvalidRemoteSource,
    #[error("remote capture payload has no valid session identifier")]
    InvalidRemotePayload,
    #[error("unsupported capture envelope schema version: {0}")]
    UnsupportedSchemaVersion(u8),
    #[error("invalid capture envelope JSON: {0}")]
    Json(#[from] serde_json::Error),
}
