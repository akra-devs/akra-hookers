use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 5")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query("ALTER TABLE activity_events ADD COLUMN capture_target TEXT")
        .execute(&mut **transaction)
        .await?;
    sqlx::query("ALTER TABLE activity_events ADD COLUMN capture_client TEXT")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_activity_events_capture_source
         ON activity_events(capture_target, capture_client, captured_at_us DESC)
         WHERE capture_target IS NOT NULL AND capture_client IS NOT NULL",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (5, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
