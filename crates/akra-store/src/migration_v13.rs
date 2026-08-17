use sqlx::{Sqlite, Transaction};

/// Allows a failed result summary to retain its source for the bounded manual
/// regeneration window. Succeeded and skipped summaries still cannot retain it.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 13")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE TABLE activity_result_summaries_v13 (
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
         );

         INSERT INTO activity_result_summaries_v13 (
             provider, provider_session_id, provider_turn_id, activity_event_id,
             source_digest, source_text, state, generation, attempt_count,
             next_attempt_at_us, lease_token, lease_expires_at_us, summary_model,
             summary_line_1, summary_line_2, summary_line_3, last_error,
             capture_target, capture_client, captured_at_us, updated_at_us, completed_at_us
         )
         SELECT provider, provider_session_id, provider_turn_id, activity_event_id,
                source_digest, source_text, state, generation, attempt_count,
                next_attempt_at_us, lease_token, lease_expires_at_us, summary_model,
                summary_line_1, summary_line_2, summary_line_3, last_error,
                capture_target, capture_client, captured_at_us, updated_at_us, completed_at_us
         FROM activity_result_summaries;

         DROP TABLE activity_result_summaries;
         ALTER TABLE activity_result_summaries_v13 RENAME TO activity_result_summaries;

         CREATE INDEX idx_activity_result_summaries_claim
             ON activity_result_summaries(
                 state, next_attempt_at_us, lease_expires_at_us, captured_at_us
             );",
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (13, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_allows_failed_sources_but_keeps_terminal_privacy_checks() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id,
                 source_digest, source_text, state, generation, summary_model,
                 captured_at_us, updated_at_us
             ) VALUES ('codex', 'failed-session', 'failed-turn', 'digest',
                       'bounded source', 'failed', 1, 'gpt-5.3-codex-spark', 1, 1)",
        )
        .execute(&store.pool)
        .await
        .expect("failed source may be retained");

        let skipped = sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id,
                 source_digest, source_text, state, generation, summary_model,
                 captured_at_us, updated_at_us
             ) VALUES ('codex', 'skipped-session', 'skipped-turn', 'digest',
                       'forbidden source', 'skipped', 1, 'gpt-5.3-codex-spark', 1, 1)",
        )
        .execute(&store.pool)
        .await;
        assert!(skipped.is_err(), "skipped summaries must scrub raw results");
    }
}
