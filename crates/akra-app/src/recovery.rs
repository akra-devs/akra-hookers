use akra_adapters::codex::{CodexAdapter, CodexCapture};
use akra_store::{ActivityStore, MAX_RESULT_SOURCE_RETENTION_US, RecordActivity, RecordResult};
use serde_json::Value;
use thiserror::Error;

use crate::spool::{CaptureEnvelope, Spool};

pub async fn drain(spool: &Spool, store: &ActivityStore) -> usize {
    let now_us = recovery_now_us();
    match now_us {
        Some(now_us) => {
            if let Err(error) =
                spool.expire_result_items_if_due(now_us, MAX_RESULT_SOURCE_RETENTION_US)
            {
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
    let mut acknowledged = Vec::new();
    let mut stored_count = 0;

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
            RecoveredCapture::Result {
                command,
                captured_at_us,
            } => {
                if now_us.is_some_and(|now_us| result_is_expired(captured_at_us, now_us)) {
                    acknowledged.push(item);
                    continue;
                }
                store.capture_result(command).await.map(|_| ())
            }
        };
        match stored {
            Ok(_) => {
                acknowledged.push(item);
                stored_count += 1;
            }
            Err(error) => eprintln!("retaining spool payload after store error: {error}"),
        }
    }

    match spool.acknowledge_batch(acknowledged) {
        Ok(_) => stored_count,
        Err(error) => {
            eprintln!("unable to acknowledge stored spool payloads: {error}");
            0
        }
    }
}

enum RecoveredCapture {
    Activity(RecordActivity),
    Result {
        command: RecordResult,
        captured_at_us: i64,
    },
}

fn decode(payload: &[u8]) -> Result<RecoveredCapture, RecoveryError> {
    let value: Value = serde_json::from_slice(payload)?;
    if value.get("schema_version").is_some() {
        let envelope = CaptureEnvelope::from_value(value)?;
        if envelope.provider() != "codex" {
            return Err(RecoveryError::UnsupportedProvider(
                envelope.provider().to_owned(),
            ));
        }
        match CodexAdapter::normalize_capture_value(envelope.payload())? {
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
                let projection = (event.activity_kind() == akra_core::ingress::ActivityKind::User)
                    .then(|| CodexAdapter::project_prompt(event.prompt()));
                let command = match envelope.capture_source() {
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
                };
                Ok(RecoveredCapture::Activity(match projection {
                    Some(projection) => command.with_prompt_projection(projection),
                    None => command,
                }))
            }
            CodexCapture::Result(event) => {
                let captured_at_us = envelope.captured_at_us();
                let command = match envelope.capture_source() {
                    Some((target, client)) => {
                        RecordResult::captured_from(event, captured_at_us, target, client)
                    }
                    None => RecordResult::captured(event, captured_at_us),
                };
                Ok(RecoveredCapture::Result {
                    command,
                    captured_at_us,
                })
            }
        }
    } else {
        let CodexCapture::Activity(event) = CodexAdapter::normalize_capture_value(&value)? else {
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

fn result_is_expired(captured_at_us: i64, now_us: i64) -> bool {
    captured_at_us <= now_us.saturating_sub(MAX_RESULT_SOURCE_RETENTION_US)
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
