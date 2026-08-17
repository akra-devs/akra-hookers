use sqlx::{Sqlite, Transaction};

/// Adds a tombstone to captured activity so a user can remove an unwanted log
/// without breaking ingest deduplication or the foreign-key graph that records
/// where it came from.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 11")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query("ALTER TABLE activity_events ADD COLUMN deleted_at_us INTEGER")
        .execute(&mut **transaction)
        .await?;
    sqlx::raw_sql(
        "CREATE INDEX idx_activity_events_active_sequence
             ON activity_events(global_sequence, id)
             WHERE deleted_at_us IS NULL;
         CREATE INDEX idx_activity_events_active_conversation_time
             ON activity_events (
                 provider,
                 provider_session_id,
                 (COALESCE(captured_at_us, first_recorded_at_us) IS NULL),
                 COALESCE(captured_at_us, first_recorded_at_us),
                 id
             )
             WHERE deleted_at_us IS NULL;",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (11, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_adds_an_idempotent_nullable_activity_tombstone() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        let deleted_column: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('activity_events') WHERE name = 'deleted_at_us'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("column");
        assert_eq!(deleted_column, 1);

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
    }
}
