use akra_core::ingress::IngressEvent;
use akra_git::ProjectOriginSnapshot;

use crate::{ActivityStore, StoreError, routing};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordActivity {
    event: IngressEvent,
    origin: ProjectOriginSnapshot,
    capture: CaptureProvenance,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum CaptureProvenance {
    Captured(i64),
    LegacyResolved,
}

impl RecordActivity {
    pub fn captured(
        event: IngressEvent,
        origin: ProjectOriginSnapshot,
        captured_at_us: i64,
    ) -> Self {
        Self {
            event,
            origin,
            capture: CaptureProvenance::Captured(captured_at_us),
        }
    }

    pub fn legacy_resolved(event: IngressEvent, origin: ProjectOriginSnapshot) -> Self {
        Self {
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

    const fn captured_at_us(&self) -> Option<i64> {
        match self.capture {
            CaptureProvenance::Captured(captured_at_us) => Some(captured_at_us),
            CaptureProvenance::LegacyResolved => None,
        }
    }

    pub(crate) const fn resolution_source(&self) -> &'static str {
        match self.capture {
            CaptureProvenance::Captured(_) => "captured",
            CaptureProvenance::LegacyResolved => "legacy_resolved",
        }
    }
}

impl ActivityStore {
    pub async fn record(&self, command: RecordActivity) -> Result<i64, StoreError> {
        let event = command.event();
        let mut transaction = self.pool.begin().await?;
        if let Some(id) = sqlx::query_scalar::<_, i64>(
            "SELECT activity_event_id FROM ingest_dedupes
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .fetch_optional(&mut *transaction)
        .await?
        {
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
        let global_sequence: i64 =
            sqlx::query_scalar("SELECT COALESCE(MAX(global_sequence), 0) + 1 FROM activity_events")
                .fetch_one(&mut *transaction)
                .await?;
        let captured_at_us = command.captured_at_us();
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
                 first_recorded_at_us, first_recorded_at_provenance, global_sequence
             ) VALUES (
                 ?, ?, ?, ?, ?, ?, ?, ?, ?,
                 CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER),
                 ?, ?
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
        .bind(global_sequence)
        .fetch_one(&mut *transaction)
        .await?;
        sqlx::query(
            "INSERT OR IGNORE INTO ingest_dedupes (
                 provider, provider_session_id, provider_turn_id, activity_event_id
             ) VALUES (?, ?, ?, ?)",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .bind(id)
        .execute(&mut *transaction)
        .await?;
        let mapped_id: i64 = sqlx::query_scalar(
            "SELECT activity_event_id FROM ingest_dedupes
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .fetch_one(&mut *transaction)
        .await?;
        if mapped_id != id {
            return Err(StoreError::Invariant(format!(
                "dedupe key mapped to activity {mapped_id} instead of newly inserted activity {id}"
            )));
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
