use sqlx::{Row, Sqlite, Transaction};

use crate::result_summaries::ResultSummaryLines;

pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 9")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    let rows = sqlx::query(
        "SELECT provider, provider_session_id, provider_turn_id,
                summary_line_1, summary_line_2, summary_line_3
         FROM activity_result_summaries
         WHERE state = 'succeeded'",
    )
    .fetch_all(&mut **transaction)
    .await?;
    for row in rows {
        let provider: String = row.try_get("provider")?;
        let session_id: String = row.try_get("provider_session_id")?;
        let turn_id: String = row.try_get("provider_turn_id")?;
        let lines = ResultSummaryLines::compact_legacy(
            row.try_get::<String, _>("summary_line_1")?,
            row.try_get::<String, _>("summary_line_2")?,
            row.try_get::<String, _>("summary_line_3")?,
        );
        match lines {
            Ok(lines) => {
                let lines = lines.as_array();
                sqlx::query(
                    "UPDATE activity_result_summaries
                     SET summary_line_1 = ?, summary_line_2 = ?, summary_line_3 = ?
                     WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
                )
                .bind(&lines[0])
                .bind(&lines[1])
                .bind(&lines[2])
                .bind(provider)
                .bind(session_id)
                .bind(turn_id)
                .execute(&mut **transaction)
                .await?;
            }
            Err(error) => {
                sqlx::query(
                    "UPDATE activity_result_summaries
                     SET state = 'failed', summary_line_1 = NULL, summary_line_2 = NULL,
                         summary_line_3 = NULL, last_error = ?
                     WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
                )
                .bind(format!("invalid legacy result summary: {error}"))
                .bind(provider)
                .bind(session_id)
                .bind(turn_id)
                .execute(&mut **transaction)
                .await?;
            }
        }
    }

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (9, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use sqlx::Row;

    use crate::{ActivityStore, MAX_RESULT_SUMMARY_CHARS};

    #[tokio::test]
    async fn migration_compacts_legacy_summaries_to_the_shared_character_budget() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id,
                 source_digest, state, generation, summary_model,
                 summary_line_1, summary_line_2, summary_line_3,
                 captured_at_us, updated_at_us
             ) VALUES ('codex', 'legacy-session', 'legacy-turn', 'digest',
                       'succeeded', 1, 'gpt-5.3-codex-spark', ?, ?, ?, 1, 1)",
        )
        .bind("가".repeat(161))
        .bind("나".repeat(166))
        .bind("다".repeat(124))
        .execute(&store.pool)
        .await
        .expect("legacy summary");
        sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id,
                 source_digest, state, generation, summary_model,
                 summary_line_1, summary_line_2, summary_line_3,
                 captured_at_us, updated_at_us
             ) VALUES ('codex', 'boundary-session', 'boundary-turn', 'digest-2',
                       'succeeded', 1, 'gpt-5.3-codex-spark', ?, ?, ?, 2, 2)",
        )
        .bind("가".repeat(60))
        .bind("나".repeat(60))
        .bind("다".repeat(60))
        .execute(&store.pool)
        .await
        .expect("boundary summary");

        let mut transaction = store.pool.begin().await.expect("transaction");
        sqlx::query("DELETE FROM schema_migrations WHERE version = 9")
            .execute(&mut *transaction)
            .await
            .expect("reset migration marker");
        super::apply(&mut transaction).await.expect("migration");
        transaction.commit().await.expect("commit");

        let row = sqlx::query(
            "SELECT summary_line_1, summary_line_2, summary_line_3
             FROM activity_result_summaries
             WHERE provider_session_id = 'legacy-session'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("compacted summary");
        let lines = [
            row.try_get::<String, _>("summary_line_1").expect("line 1"),
            row.try_get::<String, _>("summary_line_2").expect("line 2"),
            row.try_get::<String, _>("summary_line_3").expect("line 3"),
        ];
        assert_eq!(
            lines.iter().map(|line| line.chars().count()).sum::<usize>(),
            MAX_RESULT_SUMMARY_CHARS
        );
        assert!(lines.iter().all(|line| line.ends_with('…')));

        let boundary = sqlx::query(
            "SELECT summary_line_1, summary_line_2, summary_line_3
             FROM activity_result_summaries
             WHERE provider_session_id = 'boundary-session'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("boundary summary");
        assert_eq!(
            boundary
                .try_get::<String, _>("summary_line_1")
                .expect("boundary line 1"),
            "가".repeat(60)
        );

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
    }
}
