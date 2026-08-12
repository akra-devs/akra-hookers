use akra_adapters::codex::{CodexAdapter, CodexCapture};
use akra_store::{ActivityStore, MAX_RESULT_SOURCE_RETENTION_US, RecordActivity, RecordResult};
use serde_json::Value;
use thiserror::Error;

use crate::spool::{CaptureEnvelope, Spool};

pub async fn drain(spool: &Spool, store: &ActivityStore) -> usize {
    match recovery_now_us() {
        Some(now_us) => {
            if let Err(error) = spool.expire_result_items(now_us, MAX_RESULT_SOURCE_RETENTION_US) {
                eprintln!("unable to expire retained result payloads: {error}");
            }
        }
        None => eprintln!("unable to read clock for result payload retention"),
    }
    let items = match spool.recovery_candidates() {
        Ok(items) => items,
        Err(error) => {
            eprintln!("unable to list spool payloads: {error}");
            return 0;
        }
    };
    let mut drained = 0;

    for item in items {
        let payload = match spool.read(&item) {
            Ok(payload) => payload,
            Err(crate::spool::SpoolError::NonRegular | crate::spool::SpoolError::Oversized(_)) => {
                if let Err(error) = spool.defer(&item) {
                    eprintln!("unable to defer rejected spool item: {error}");
                }
                continue;
            }
            Err(error) => {
                eprintln!("retaining spool payload after read error: {error}");
                continue;
            }
        };
        let command = match decode(&payload) {
            Ok(command) => command,
            Err(error) => {
                eprintln!("deferring invalid spool payload until restart: {error}");
                if let Err(error) = spool.defer(&item) {
                    eprintln!("unable to defer rejected spool item: {error}");
                }
                continue;
            }
        };

        let stored = match command {
            RecoveredCapture::Activity(command) => store.record(command).await.map(|_| ()),
            RecoveredCapture::Result(command) => store.capture_result(command).await.map(|_| ()),
        };
        match stored {
            Ok(_) => match spool.acknowledge(item) {
                Ok(()) => drained += 1,
                Err(error) => eprintln!("unable to acknowledge spool payload: {error}"),
            },
            Err(error) => eprintln!("retaining spool payload after store error: {error}"),
        }
    }

    drained
}

enum RecoveredCapture {
    Activity(RecordActivity),
    Result(RecordResult),
}

fn decode(payload: &[u8]) -> Result<RecoveredCapture, RecoveryError> {
    let value: Value = serde_json::from_slice(payload)?;
    if value.get("schema_version").is_some() {
        let envelope = CaptureEnvelope::decode(payload)?;
        if envelope.provider() != "codex" {
            return Err(RecoveryError::UnsupportedProvider(
                envelope.provider().to_owned(),
            ));
        }
        let provider_payload = serde_json::to_string(envelope.payload())?;
        match CodexAdapter::normalize_capture(&provider_payload)? {
            CodexCapture::Activity(event) => {
                let (activity_kind, agent_id, agent_type) = envelope.activity_context();
                let event = if activity_kind == akra_core::ingress::ActivityKind::User {
                    event
                } else {
                    event.with_activity_context(
                        activity_kind,
                        agent_id.map(ToOwned::to_owned),
                        agent_type.map(ToOwned::to_owned),
                    )?
                };
                Ok(RecoveredCapture::Activity(
                    match envelope.capture_source() {
                        Some((target, client)) => RecordActivity::captured_from(
                            event,
                            envelope.origin().clone(),
                            envelope.captured_at_us(),
                            target,
                            client,
                        ),
                        None => RecordActivity::captured(
                            event,
                            envelope.origin().clone(),
                            envelope.captured_at_us(),
                        ),
                    },
                ))
            }
            CodexCapture::Result(event) => {
                Ok(RecoveredCapture::Result(match envelope.capture_source() {
                    Some((target, client)) => RecordResult::captured_from(
                        event,
                        envelope.captured_at_us(),
                        target,
                        client,
                    ),
                    None => RecordResult::captured(event, envelope.captured_at_us()),
                }))
            }
        }
    } else {
        let input = std::str::from_utf8(payload)?;
        let CodexCapture::Activity(event) = CodexAdapter::normalize_capture(input)? else {
            return Err(RecoveryError::LegacyResult);
        };
        let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(
            event.cwd(),
        ))?
        .origin;
        Ok(RecoveredCapture::Activity(RecordActivity::legacy_resolved(
            event, origin,
        )))
    }
}

fn recovery_now_us() -> Option<i64> {
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?;
    i64::try_from(elapsed.as_micros()).ok()
}

#[derive(Debug, Error)]
enum RecoveryError {
    #[error("invalid spool JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid spool UTF-8: {0}")]
    Utf8(#[from] std::str::Utf8Error),
    #[error("invalid capture envelope: {0}")]
    Envelope(#[from] crate::spool::CaptureEnvelopeError),
    #[error("unsupported capture provider: {0}")]
    UnsupportedProvider(String),
    #[error("invalid Codex payload: {0}")]
    Codex(#[from] akra_adapters::codex::CodexAdapterError),
    #[error("invalid captured activity context: {0}")]
    Ingress(#[from] akra_core::ingress::IngressError),
    #[error("unable to resolve legacy spool origin: {0}")]
    Identity(#[from] akra_git::IdentityError),
    #[error("legacy result payloads are unsupported")]
    LegacyResult,
}
