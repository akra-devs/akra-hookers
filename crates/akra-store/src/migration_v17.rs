use sqlx::{Sqlite, Transaction};

/// Adds a stable discriminator for the immediately preceding result-summary failure.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 17")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::query("ALTER TABLE activity_result_summaries ADD COLUMN last_error_code TEXT")
        .execute(&mut **transaction)
        .await?;
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (17, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_adds_an_idempotent_result_failure_code() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        let column: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM pragma_table_info('activity_result_summaries')
             WHERE name = 'last_error_code'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("column");
        assert_eq!(column, 1);

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
    }
}
