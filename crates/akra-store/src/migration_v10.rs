use sqlx::{Sqlite, Transaction};

/// Adds opt-in contextual prompt summary state without backfilling historic
/// activities.  Prompt text is always kept in `activity_events`; this table
/// stores only the derived projection and a bounded model result.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 10")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE TABLE provider_summary_settings (
             provider TEXT PRIMARY KEY,
             prompt_summary_mode TEXT NOT NULL DEFAULT 'off'
                 CHECK(prompt_summary_mode IN ('off', 'smart')),
             updated_at_us INTEGER NOT NULL
         );

         CREATE TABLE activity_prompt_summaries (
             activity_event_id INTEGER PRIMARY KEY
                 REFERENCES activity_events(id) ON DELETE CASCADE,
             state TEXT NOT NULL
                 CHECK(state IN (
                     'passthrough', 'waiting_context', 'pending', 'running',
                     'retry_wait', 'succeeded', 'failed'
                 )),
             projection_kind TEXT NOT NULL
                 CHECK(projection_kind IN ('raw', 'codex_wrapper_removed')),
             projected_prompt TEXT NOT NULL,
             projection_version INTEGER NOT NULL,
             policy_at_capture TEXT NOT NULL
                 CHECK(policy_at_capture IN ('off', 'smart')),
             summary_text TEXT,
             used_previous_result INTEGER NOT NULL DEFAULT 0
                 CHECK(used_previous_result IN (0, 1)),
             context_activity_event_id INTEGER
                 REFERENCES activity_events(id) ON DELETE SET NULL,
             context_result_generation INTEGER,
             source_digest TEXT NOT NULL,
             generation INTEGER NOT NULL DEFAULT 1,
             summary_model TEXT NOT NULL,
             attempt_count INTEGER NOT NULL DEFAULT 0,
             lease_token TEXT,
             lease_expires_at_us INTEGER,
             next_attempt_at_us INTEGER NOT NULL DEFAULT 0,
             last_error_code TEXT,
             created_at_us INTEGER NOT NULL,
             updated_at_us INTEGER NOT NULL,
             CHECK(
                 state != 'succeeded'
                 OR (summary_text IS NOT NULL AND length(trim(summary_text)) > 0)
             )
         );

         CREATE INDEX activity_prompt_summaries_claim
             ON activity_prompt_summaries(state, next_attempt_at_us, updated_at_us);
         CREATE INDEX activity_prompt_summaries_digest
             ON activity_prompt_summaries(source_digest, state);
         CREATE INDEX activity_prompt_summaries_context
             ON activity_prompt_summaries(context_activity_event_id, context_result_generation);",
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (10, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_is_idempotent_and_does_not_backfill_historic_activities() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM activity_prompt_summaries")
            .fetch_one(&store.pool)
            .await
            .expect("count");
        assert_eq!(count, 0);

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
    }
}
