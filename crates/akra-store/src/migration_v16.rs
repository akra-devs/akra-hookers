use sqlx::{Sqlite, Transaction};

/// Adds durable per-call Codex token telemetry and the quota circuit state.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 16")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE TABLE codex_exec_calls (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             operation TEXT NOT NULL
                 CHECK(operation IN ('result_summary', 'prompt_summary', 'work_curation')),
             activity_event_id INTEGER REFERENCES activity_events(id) ON DELETE SET NULL,
             model TEXT NOT NULL CHECK(trim(model) != ''),
             capture_target TEXT,
             attempt_number INTEGER NOT NULL CHECK(attempt_number >= 1),
             source_chars INTEGER NOT NULL CHECK(source_chars >= 0),
             submitted_source_chars INTEGER NOT NULL CHECK(submitted_source_chars >= 0),
             prompt_chars INTEGER NOT NULL CHECK(prompt_chars >= 0),
             started_at_us INTEGER NOT NULL,
             completed_at_us INTEGER NOT NULL CHECK(completed_at_us >= started_at_us),
             status TEXT NOT NULL
                 CHECK(status IN ('succeeded', 'failed', 'timed_out', 'quota_limited')),
             exit_code INTEGER,
             thread_id TEXT,
             input_tokens INTEGER CHECK(input_tokens IS NULL OR input_tokens >= 0),
             cached_input_tokens INTEGER
                 CHECK(cached_input_tokens IS NULL OR cached_input_tokens >= 0),
             output_tokens INTEGER CHECK(output_tokens IS NULL OR output_tokens >= 0),
             reasoning_output_tokens INTEGER
                 CHECK(reasoning_output_tokens IS NULL OR reasoning_output_tokens >= 0),
             error_code TEXT,
             error_message TEXT,
             quota_retry_at_us INTEGER,
             CHECK(
                 (input_tokens IS NULL AND cached_input_tokens IS NULL
                    AND output_tokens IS NULL AND reasoning_output_tokens IS NULL)
                 OR
                 (input_tokens IS NOT NULL AND cached_input_tokens IS NOT NULL
                    AND output_tokens IS NOT NULL AND reasoning_output_tokens IS NOT NULL)
             ),
             CHECK(cached_input_tokens IS NULL OR cached_input_tokens <= input_tokens),
             CHECK(
                 (status = 'quota_limited' AND quota_retry_at_us IS NOT NULL)
                 OR (status != 'quota_limited' AND quota_retry_at_us IS NULL)
             )
         );
         CREATE INDEX codex_exec_calls_started
             ON codex_exec_calls(started_at_us, id);
         CREATE INDEX codex_exec_calls_model_operation
             ON codex_exec_calls(model, operation, started_at_us);
         CREATE INDEX codex_exec_calls_activity
             ON codex_exec_calls(activity_event_id, started_at_us);

         CREATE TABLE codex_quota_circuits (
             model TEXT PRIMARY KEY CHECK(trim(model) != ''),
             opened_at_us INTEGER NOT NULL,
             retry_at_us INTEGER NOT NULL CHECK(retry_at_us > opened_at_us),
             last_error TEXT NOT NULL,
             updated_at_us INTEGER NOT NULL
         );",
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (16, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_creates_usage_and_circuit_tables_idempotently() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        for table in ["codex_exec_calls", "codex_quota_circuits"] {
            assert_eq!(
                sqlx::query_scalar::<_, i64>(
                    "SELECT COUNT(*) FROM sqlite_schema WHERE type = 'table' AND name = ?",
                )
                .bind(table)
                .fetch_one(&store.pool)
                .await
                .expect("table count"),
                1,
                "missing {table}"
            );
        }

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM schema_migrations WHERE version = 16",
            )
            .fetch_one(&store.pool)
            .await
            .expect("migration marker"),
            1
        );
    }
}
