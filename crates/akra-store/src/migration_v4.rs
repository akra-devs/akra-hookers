use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 4")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query("ALTER TABLE canvas_nodes ADD COLUMN deleted_at_us INTEGER")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_canvas_nodes_active_activity
         ON canvas_nodes(activity_event_id)
         WHERE deleted_at_us IS NULL",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (4, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
