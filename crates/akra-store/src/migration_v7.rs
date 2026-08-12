use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 7")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    // Managed Windows hooks resolve ordinary App/CLI sessions to an explicit
    // client. Unattributed managed captures are Codex's ephemeral internal work.
    sqlx::query(
        "UPDATE activity_events
         SET activity_kind = 'internal'
         WHERE provider = 'codex'
           AND activity_kind = 'user'
           AND capture_target IS NOT NULL
           AND capture_client = 'unknown'",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (7, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
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
    async fn migration_reclassifies_only_unattributed_managed_capture() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("current directory");
        for (turn, client) in [("internal", "unknown"), ("app-user", "app")] {
            let event = IngressEvent::try_new(
                "codex",
                turn,
                turn,
                cwd.to_string_lossy(),
                format!("prompt {turn}"),
                None,
            )
            .expect("event");
            let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
                .expect("origin")
                .origin;
            store
                .record(RecordActivity::captured_from(
                    event,
                    origin,
                    1,
                    "windows-native",
                    client,
                ))
                .await
                .expect("record");
        }

        let mut transaction = store.pool.begin().await.expect("transaction");
        sqlx::query("DELETE FROM schema_migrations WHERE version = 7")
            .execute(&mut *transaction)
            .await
            .expect("reset migration marker");
        super::apply(&mut transaction).await.expect("migration");
        transaction.commit().await.expect("commit");

        let summaries = store
            .activity_summaries(ActivityScope::All)
            .await
            .expect("summaries");
        assert_eq!(summaries[0].activity_kind, ActivityKind::Internal);
        assert_eq!(summaries[1].activity_kind, ActivityKind::User);
    }
}
