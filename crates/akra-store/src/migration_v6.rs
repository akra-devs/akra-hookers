use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 6")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query(
        "ALTER TABLE activity_events
         ADD COLUMN activity_kind TEXT NOT NULL DEFAULT 'user'
         CHECK(activity_kind IN ('user', 'subagent', 'internal'))",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query("ALTER TABLE activity_events ADD COLUMN agent_id TEXT")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("ALTER TABLE activity_events ADD COLUMN agent_type TEXT")
        .execute(&mut **transaction)
        .await?;

    // Older desktop ambient-suggestion captures can be identified by their
    // submitted application directory without inspecting prompt text.
    backfill_internal_activity(transaction).await?;
    sqlx::query(
        "CREATE INDEX idx_activity_events_kind_sequence
         ON activity_events(activity_kind, global_sequence DESC, id DESC)",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (6, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn backfill_internal_activity(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "UPDATE activity_events
         SET activity_kind = 'internal'
         WHERE provider = 'codex'
           AND instr(
               rtrim(lower(replace(submitted_cwd, char(92), '/')), '/'),
               '/windowsapps/openai.codex_'
           ) > 0
           AND substr(
               rtrim(lower(replace(submitted_cwd, char(92), '/')), '/'),
               -4
           ) = '/app'",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use akra_core::ingress::{ActivityKind, IngressEvent};
    use akra_git::ProjectIdentity;

    use crate::{ActivityScope, ActivityStore, RecordActivity, StoreError};

    #[tokio::test]
    async fn backfill_uses_codex_installation_path_without_prompt_matching() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = r"C:\Program Files\WindowsApps\OpenAI.Codex_1.2.3_x64__8wekyb3d8bbwe\app";
        let event = IngressEvent::try_new(
            "codex",
            "ambient-session",
            "ambient-turn",
            cwd,
            "prompt wording is intentionally irrelevant",
            None,
        )
        .expect("event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(cwd))
            .expect("origin")
            .origin;
        store
            .record(RecordActivity::captured(event, origin, 1))
            .await
            .expect("record");

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::backfill_internal_activity(&mut transaction)
            .await
            .expect("backfill");
        transaction.commit().await.expect("commit");

        let summaries = store
            .activity_summaries(ActivityScope::All)
            .await
            .expect("summaries");
        assert_eq!(summaries[0].activity_kind, ActivityKind::Internal);
    }

    #[tokio::test]
    async fn subagent_identity_is_rejected_at_the_store_boundary() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("current directory");
        let event = IngressEvent::try_new(
            "codex",
            "parent-session",
            "parent-turn:subagent:agent-7",
            cwd.to_string_lossy(),
            "Subagent started: reviewer",
            None,
        )
        .expect("event")
        .with_activity_context(
            ActivityKind::Subagent,
            Some("agent-7".to_owned()),
            Some("reviewer".to_owned()),
        )
        .expect("context");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let error = store
            .record(RecordActivity::captured(event, origin, 2))
            .await
            .expect_err("subagent activity must not be stored");
        assert!(matches!(error, StoreError::SubagentActivityDisabled));
        assert!(
            store
                .activity_summaries(ActivityScope::All)
                .await
                .expect("summaries")
                .is_empty()
        );
    }
}
