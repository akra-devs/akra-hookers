use sqlx::{Sqlite, Transaction};

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 8")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query(
        "CREATE TABLE activity_result_summaries (
             provider TEXT NOT NULL,
             provider_session_id TEXT NOT NULL,
             provider_turn_id TEXT NOT NULL,
             activity_event_id INTEGER UNIQUE
                 REFERENCES activity_events(id) ON DELETE CASCADE,
             source_digest TEXT NOT NULL,
             source_text TEXT,
             state TEXT NOT NULL
                 CHECK(state IN (
                     'pending', 'running', 'retry_wait',
                     'succeeded', 'failed', 'skipped'
                 )),
             generation INTEGER NOT NULL CHECK(generation >= 1),
             attempt_count INTEGER NOT NULL DEFAULT 0 CHECK(attempt_count >= 0),
             next_attempt_at_us INTEGER NOT NULL DEFAULT 0,
             lease_token TEXT,
             lease_expires_at_us INTEGER,
             summary_model TEXT NOT NULL,
             summary_line_1 TEXT,
             summary_line_2 TEXT,
             summary_line_3 TEXT,
             last_error TEXT,
             capture_target TEXT,
             capture_client TEXT,
             captured_at_us INTEGER NOT NULL,
             updated_at_us INTEGER NOT NULL,
             completed_at_us INTEGER,
             PRIMARY KEY(provider, provider_session_id, provider_turn_id),
             CHECK(
                 (state = 'running' AND lease_token IS NOT NULL
                    AND lease_expires_at_us IS NOT NULL)
                 OR
                 (state != 'running' AND lease_token IS NULL
                    AND lease_expires_at_us IS NULL)
             ),
             CHECK(
                 (state = 'succeeded'
                    AND source_text IS NULL
                    AND summary_line_1 IS NOT NULL
                    AND summary_line_2 IS NOT NULL
                    AND summary_line_3 IS NOT NULL)
                 OR
                 (state != 'succeeded'
                    AND summary_line_1 IS NULL
                    AND summary_line_2 IS NULL
                    AND summary_line_3 IS NULL)
             ),
             CHECK(
                 state NOT IN ('skipped', 'succeeded')
                 OR source_text IS NULL
             )
         )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "CREATE INDEX idx_activity_result_summaries_claim
         ON activity_result_summaries(
             state, next_attempt_at_us, lease_expires_at_us, captured_at_us
         )",
    )
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (8, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
