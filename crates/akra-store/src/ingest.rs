use akra_core::{
    ingress::{ActivityKind, IngressEvent},
    prompt_projection::PromptProjection,
};
use akra_git::ProjectOriginSnapshot;
use sqlx::Row;

use crate::{ActivityStore, StoreError, routing};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordActivity {
    event: IngressEvent,
    origin: ProjectOriginSnapshot,
    capture: CaptureProvenance,
    prompt_projection: PromptProjection,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CaptureProvenance {
    Captured {
        captured_at_us: i64,
        source: Option<CaptureSource>,
    },
    LegacyResolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CaptureSource {
    target: String,
    client: String,
}

impl RecordActivity {
    pub fn captured(
        event: IngressEvent,
        origin: ProjectOriginSnapshot,
        captured_at_us: i64,
    ) -> Self {
        Self {
            prompt_projection: PromptProjection::raw(event.prompt()),
            event,
            origin,
            capture: CaptureProvenance::Captured {
                captured_at_us,
                source: None,
            },
        }
    }

    pub fn captured_from(
        event: IngressEvent,
        origin: ProjectOriginSnapshot,
        captured_at_us: i64,
        target: impl Into<String>,
        client: impl Into<String>,
    ) -> Self {
        Self {
            prompt_projection: PromptProjection::raw(event.prompt()),
            event,
            origin,
            capture: CaptureProvenance::Captured {
                captured_at_us,
                source: Some(CaptureSource {
                    target: target.into(),
                    client: client.into(),
                }),
            },
        }
    }

    pub fn legacy_resolved(event: IngressEvent, origin: ProjectOriginSnapshot) -> Self {
        Self {
            prompt_projection: PromptProjection::raw(event.prompt()),
            event,
            origin,
            capture: CaptureProvenance::LegacyResolved,
        }
    }

    pub(crate) fn event(&self) -> &IngressEvent {
        &self.event
    }

    pub(crate) fn origin(&self) -> &ProjectOriginSnapshot {
        &self.origin
    }

    pub fn with_prompt_projection(mut self, projection: PromptProjection) -> Self {
        self.prompt_projection = projection;
        self
    }

    pub(crate) fn prompt_projection(&self) -> &PromptProjection {
        &self.prompt_projection
    }

    const fn captured_at_us(&self) -> Option<i64> {
        match &self.capture {
            CaptureProvenance::Captured { captured_at_us, .. } => Some(*captured_at_us),
            CaptureProvenance::LegacyResolved => None,
        }
    }

    fn capture_source(&self) -> Option<(&str, &str)> {
        match &self.capture {
            CaptureProvenance::Captured {
                source: Some(source),
                ..
            } => Some((&source.target, &source.client)),
            CaptureProvenance::Captured { source: None, .. }
            | CaptureProvenance::LegacyResolved => None,
        }
    }

    pub(crate) const fn resolution_source(&self) -> &'static str {
        match &self.capture {
            CaptureProvenance::Captured { .. } => "captured",
            CaptureProvenance::LegacyResolved => "legacy_resolved",
        }
    }
}

impl ActivityStore {
    pub async fn record(&self, command: RecordActivity) -> Result<i64, StoreError> {
        let event = command.event();
        if event.activity_kind() == ActivityKind::Subagent {
            return Err(StoreError::SubagentActivityDisabled);
        }
        let mut transaction = self.pool.begin().await?;
        if let Some(existing) = sqlx::query(
            "SELECT dedupes.activity_event_id, activities.activity_kind
             FROM ingest_dedupes AS dedupes
             JOIN activity_events AS activities ON activities.id = dedupes.activity_event_id
             WHERE dedupes.provider = ? AND dedupes.provider_session_id = ?
               AND dedupes.provider_turn_id = ?",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .fetch_optional(&mut *transaction)
        .await?
        {
            let id: i64 = existing.try_get("activity_event_id")?;
            let activity_kind = stored_activity_kind(existing.try_get("activity_kind")?)?;
            crate::result_summaries::link_activity(
                &mut transaction,
                id,
                activity_kind,
                event.provider().as_str(),
                event.session_id(),
                event.turn_id(),
            )
            .await?;
            // A replay can point at activity recorded before the prompt-summary
            // migration. Do not turn that dedupe lookup into a historical
            // backfill; only the original insert initializes this derived row.
            transaction.commit().await?;
            return Ok(id);
        }

        let origin = routing::ensure_origin(&mut transaction, &command).await?;
        let project_id = routing::assignment_for(
            &mut transaction,
            &origin,
            event.provider().as_str(),
            event.session_id(),
        )
        .await?;
        let captured_at_us = command.captured_at_us();
        let (capture_target, capture_client) = command
            .capture_source()
            .map_or((None, None), |(target, client)| {
                (Some(target), Some(client))
            });
        let captured_provenance = captured_at_us.map(|_| "captured");
        let first_recorded_provenance = if captured_at_us.is_some() {
            "captured"
        } else {
            "legacy_recorded"
        };
        let id: i64 = sqlx::query_scalar(
            "INSERT INTO activity_events (
                 provider, provider_session_id, provider_turn_id, project_identity, prompt,
                 origin_id, submitted_cwd, captured_at_us, captured_at_provenance,
                 first_recorded_at_us, first_recorded_at_provenance, global_sequence,
                 capture_target, capture_client, activity_kind, agent_id, agent_type
             ) VALUES (
                 ?, ?, ?, ?, ?, ?, ?, ?, ?,
                 CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER),
                 ?, (SELECT COALESCE(MAX(global_sequence), 0) + 1 FROM activity_events),
                 ?, ?, ?, ?, ?
             )
             RETURNING id",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .bind(event.cwd())
        .bind(event.prompt())
        .bind(origin.id)
        .bind(event.cwd())
        .bind(captured_at_us)
        .bind(captured_provenance)
        .bind(first_recorded_provenance)
        .bind(capture_target)
        .bind(capture_client)
        .bind(event.activity_kind().as_str())
        .bind(event.agent_id())
        .bind(event.agent_type())
        .fetch_one(&mut *transaction)
        .await?;
        let inserted_mapping = sqlx::query_scalar::<_, i64>(
            "INSERT INTO ingest_dedupes (
                 provider, provider_session_id, provider_turn_id, activity_event_id
             ) VALUES (?, ?, ?, ?)
             ON CONFLICT(provider, provider_session_id, provider_turn_id) DO NOTHING
             RETURNING activity_event_id",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .bind(id)
        .fetch_optional(&mut *transaction)
        .await?;
        let mapped_id = match inserted_mapping {
            Some(mapped_id) => mapped_id,
            None => {
                sqlx::query_scalar(
                    "SELECT activity_event_id FROM ingest_dedupes
                     WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
                )
                .bind(event.provider().as_str())
                .bind(event.session_id())
                .bind(event.turn_id())
                .fetch_one(&mut *transaction)
                .await?
            }
        };
        if mapped_id != id {
            return Err(StoreError::Invariant(format!(
                "dedupe key mapped to activity {mapped_id} instead of newly inserted activity {id}"
            )));
        }
        let summary_now_us = captured_at_us.unwrap_or_else(recorded_now_us);
        crate::result_summaries::link_activity(
            &mut transaction,
            id,
            event.activity_kind(),
            event.provider().as_str(),
            event.session_id(),
            event.turn_id(),
        )
        .await?;
        crate::prompt_summaries::initialize_new_prompt_summary_in_transaction(
            &mut transaction,
            id,
            event.provider().as_str(),
            event.prompt(),
            event.activity_kind(),
            command.prompt_projection(),
            summary_now_us,
        )
        .await?;
        if event.activity_kind() == akra_core::ingress::ActivityKind::User {
            crate::prompt_summaries::reconcile_successor_after_activity(
                &mut transaction,
                id,
                summary_now_us,
            )
            .await?;
        }
        if let Some(project_id) = project_id {
            sqlx::query(
                "INSERT INTO activity_project_assignments (
                     activity_event_id, project_id, updated_at_us
                 ) VALUES (
                     ?, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
                 )",
            )
            .bind(id)
            .bind(project_id)
            .execute(&mut *transaction)
            .await?;
        }
        sqlx::query("INSERT INTO canvas_nodes (activity_event_id) VALUES (?)")
            .bind(id)
            .execute(&mut *transaction)
            .await?;
        crate::canvas::bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(id)
    }
}

fn stored_activity_kind(value: String) -> Result<ActivityKind, StoreError> {
    ActivityKind::from_storage(&value)
        .ok_or_else(|| StoreError::Invariant(format!("invalid activity kind: {value}")))
}

fn recorded_now_us() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_micros()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use akra_core::ingress::IngressEvent;
    use akra_git::ProjectIdentity;

    use super::{ActivityStore, RecordActivity};

    #[tokio::test]
    async fn a_dedupe_replay_never_backfills_a_historic_prompt_summary() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("cwd");
        let prompt = "가".repeat(300);
        let event = IngressEvent::try_new(
            "codex",
            "historic-session",
            "historic-turn",
            cwd.to_string_lossy(),
            &prompt,
            None,
        )
        .expect("event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let activity_id = store
            .record(RecordActivity::captured(event.clone(), origin.clone(), 10))
            .await
            .expect("initial activity");
        sqlx::query("DELETE FROM activity_prompt_summaries WHERE activity_event_id = ?")
            .bind(activity_id)
            .execute(&store.pool)
            .await
            .expect("simulate pre-migration activity");
        store
            .set_prompt_summary_policy("codex", crate::PromptSummaryPolicy::Smart)
            .await
            .expect("enable smart policy");

        let replayed_id = store
            .record(RecordActivity::captured(event, origin, 10))
            .await
            .expect("dedupe replay");

        assert_eq!(replayed_id, activity_id);
        assert!(
            store
                .prompt_summary(activity_id)
                .await
                .expect("prompt summary")
                .is_none()
        );
    }
}
