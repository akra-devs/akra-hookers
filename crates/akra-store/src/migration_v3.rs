use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 3")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }
    sqlx::query(
        "CREATE TABLE project_id_allocator (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             next_id INTEGER NOT NULL CHECK (next_id > 0)
         )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO project_id_allocator (singleton, next_id)
         SELECT 1, COALESCE(MAX(id), 0) + 1 FROM projects",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "CREATE TABLE canvas_state_revision (
             singleton INTEGER PRIMARY KEY CHECK (singleton = 1),
             revision INTEGER NOT NULL CHECK (revision >= 0)
         )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query("INSERT INTO canvas_state_revision (singleton, revision) VALUES (1, 0)")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "CREATE INDEX idx_activity_events_conversation_time
         ON activity_events (
             provider,
             provider_session_id,
             (COALESCE(captured_at_us, first_recorded_at_us) IS NULL),
             COALESCE(captured_at_us, first_recorded_at_us),
             id
         )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (3, CAST(unixepoch('now') AS INTEGER) * 1000000)",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
