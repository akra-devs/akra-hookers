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

    use crate::{ActivityScope, ActivityStore, RecordActivity};

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
    async fn subagent_identity_round_trips_through_summary_and_detail() {
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
        let id = store
            .record(RecordActivity::captured(event, origin, 2))
            .await
            .expect("record");

        let summary = store
            .activity_summaries(ActivityScope::All)
            .await
            .expect("summaries")
            .pop()
            .expect("summary");
        assert_eq!(summary.activity_kind, ActivityKind::Subagent);
        let detail = store.activity_detail(id).await.expect("detail");
        assert_eq!(detail.activity_kind, ActivityKind::Subagent);
        assert_eq!(detail.technical.agent_id.as_deref(), Some("agent-7"));
        assert_eq!(detail.technical.agent_type.as_deref(), Some("reviewer"));
    }
}
