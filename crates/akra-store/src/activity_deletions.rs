use crate::{ActivityStore, StoreError, canvas::bump_canvas_revision};

impl ActivityStore {
    /// Tombstones one active activity and removes its live canvas projection.
    /// The source row remains in place so dedupe and relational provenance do
    /// not become inconsistent after a user-initiated deletion.
    pub async fn delete_activity(&self, activity_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        let deleted_at_us: i64 = sqlx::query_scalar(
            "SELECT CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)",
        )
        .fetch_one(&mut *transaction)
        .await?;
        let deleted = sqlx::query(
            "UPDATE activity_events
             SET deleted_at_us = ?
             WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(deleted_at_us)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?;
        if deleted.rows_affected() == 0 {
            return Err(StoreError::ActivityNotFound(activity_id));
        }

        sqlx::query(
            "DELETE FROM canvas_edges
             WHERE source_node_id IN (
                 SELECT id FROM canvas_nodes WHERE activity_event_id = ?
             ) OR target_node_id IN (
                 SELECT id FROM canvas_nodes WHERE activity_event_id = ?
             )",
        )
        .bind(activity_id)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE canvas_nodes
             SET deleted_at_us = ?
             WHERE activity_event_id = ? AND deleted_at_us IS NULL",
        )
        .bind(deleted_at_us)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
}
