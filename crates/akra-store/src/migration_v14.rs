use sqlx::{Sqlite, Transaction};

/// Stores raw prompt projections by reference to `activity_events.prompt`.
/// Only projections that differ from the captured prompt keep their own text.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 14")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE TABLE activity_prompt_summaries_v14 (
             activity_event_id INTEGER PRIMARY KEY
                 REFERENCES activity_events(id) ON DELETE CASCADE,
             state TEXT NOT NULL
                 CHECK(state IN (
                     'passthrough', 'waiting_context', 'pending', 'running',
                     'retry_wait', 'succeeded', 'failed'
                 )),
             projection_kind TEXT NOT NULL
                 CHECK(projection_kind IN ('raw', 'codex_wrapper_removed')),
             projected_prompt TEXT,
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
             CHECK(projection_kind = 'raw' OR projected_prompt IS NOT NULL),
             CHECK(
                 state != 'succeeded'
                 OR (summary_text IS NOT NULL AND length(trim(summary_text)) > 0)
             )
         );

         INSERT INTO activity_prompt_summaries_v14 (
             activity_event_id, state, projection_kind, projected_prompt,
             projection_version, policy_at_capture, summary_text, used_previous_result,
             context_activity_event_id, context_result_generation, source_digest,
             generation, summary_model, attempt_count, lease_token,
             lease_expires_at_us, next_attempt_at_us, last_error_code,
             created_at_us, updated_at_us
         )
         SELECT summaries.activity_event_id, summaries.state, summaries.projection_kind,
                CASE
                    WHEN summaries.projection_kind = 'raw'
                     AND summaries.projected_prompt = activities.prompt
                    THEN NULL
                    ELSE summaries.projected_prompt
                END,
                summaries.projection_version, summaries.policy_at_capture,
                summaries.summary_text, summaries.used_previous_result,
                summaries.context_activity_event_id, summaries.context_result_generation,
                summaries.source_digest, summaries.generation, summaries.summary_model,
                summaries.attempt_count, summaries.lease_token,
                summaries.lease_expires_at_us, summaries.next_attempt_at_us,
                summaries.last_error_code, summaries.created_at_us, summaries.updated_at_us
         FROM activity_prompt_summaries AS summaries
         LEFT JOIN activity_events AS activities
           ON activities.id = summaries.activity_event_id;

         DROP TABLE activity_prompt_summaries;
         ALTER TABLE activity_prompt_summaries_v14 RENAME TO activity_prompt_summaries;

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
         VALUES (14, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::Row;

    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_nulls_raw_duplicates_and_preserves_derived_projections() {
        let store = ActivityStore::in_memory().await.expect("store");
        sqlx::raw_sql(
            "PRAGMA foreign_keys = ON;
             CREATE TABLE schema_migrations (
                 version INTEGER PRIMARY KEY,
                 applied_at_us INTEGER NOT NULL
             );
             CREATE TABLE activity_events (
                 id INTEGER PRIMARY KEY,
                 prompt TEXT NOT NULL
             );
             INSERT INTO activity_events (id, prompt) VALUES
                 (1, 'captured prompt'),
                 (2, 'captured prompt'),
                 (3, 'wrapped prompt');",
        )
        .execute(&store.pool)
        .await
        .expect("base schema");
        let mut transaction = store.pool.begin().await.expect("v10 transaction");
        crate::migration_v10::apply(&mut transaction)
            .await
            .expect("v10 schema");
        transaction.commit().await.expect("v10 commit");
        sqlx::query(
            "INSERT INTO activity_prompt_summaries (
                 activity_event_id, state, projection_kind, projected_prompt,
                 projection_version, policy_at_capture, source_digest,
                 summary_model, created_at_us, updated_at_us
             ) VALUES
                 (1, 'passthrough', 'raw', 'captured prompt', 1, 'off',
                  'raw-digest', 'model', 1, 1),
                 (2, 'passthrough', 'raw', 'alternate raw prompt', 1, 'off',
                  'alternate-digest', 'model', 1, 1),
                 (3, 'passthrough', 'codex_wrapper_removed', 'derived prompt', 1, 'off',
                  'derived-digest', 'model', 1, 1)",
        )
        .execute(&store.pool)
        .await
        .expect("legacy summary rows");

        let mut transaction = store.pool.begin().await.expect("v14 transaction");
        super::apply(&mut transaction).await.expect("v14 migration");
        transaction.commit().await.expect("v14 commit");

        let rows = sqlx::query(
            "SELECT activity_event_id, projected_prompt
             FROM activity_prompt_summaries ORDER BY activity_event_id",
        )
        .fetch_all(&store.pool)
        .await
        .expect("migrated projections");
        assert_eq!(
            rows[0]
                .try_get::<Option<String>, _>("projected_prompt")
                .unwrap(),
            None
        );
        assert_eq!(
            rows[1]
                .try_get::<Option<String>, _>("projected_prompt")
                .unwrap()
                .as_deref(),
            Some("alternate raw prompt")
        );
        assert_eq!(
            rows[2]
                .try_get::<Option<String>, _>("projected_prompt")
                .unwrap()
                .as_deref(),
            Some("derived prompt")
        );
        let projected_prompt_not_null: i64 = sqlx::query_scalar(
            "SELECT \"notnull\" FROM pragma_table_info('activity_prompt_summaries')
             WHERE name = 'projected_prompt'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("projected prompt schema");
        assert_eq!(projected_prompt_not_null, 0);
        let indexes = sqlx::query_scalar::<_, String>(
            "SELECT name FROM sqlite_master
             WHERE type = 'index' AND tbl_name = 'activity_prompt_summaries'
             ORDER BY name",
        )
        .fetch_all(&store.pool)
        .await
        .expect("summary indexes");
        assert_eq!(
            indexes,
            vec![
                "activity_prompt_summaries_claim",
                "activity_prompt_summaries_context",
                "activity_prompt_summaries_digest",
            ]
        );
        assert!(
            sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
                .fetch_optional(&store.pool)
                .await
                .expect("foreign key check")
                .is_none()
        );

        let mut transaction = store.pool.begin().await.expect("repeat transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("repeat commit");
    }
}
