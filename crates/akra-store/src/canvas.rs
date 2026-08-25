use sqlx::{Sqlite, Transaction};

use crate::{
    ActivityStore, CanvasEdgeSummary, CanvasNodeSummary, StoreError,
    work_curation::soft_delete_activity_in,
};

pub(crate) const CANVAS_ORIGIN_X: f64 = 64.0;
pub(crate) const CANVAS_ORIGIN_Y: f64 = 64.0;
const CANVAS_HORIZONTAL_STEP: f64 = 336.0;
const CANVAS_VERTICAL_STEP: f64 = 220.0;

pub(crate) fn next_compact_canvas_position(occupied: &[(f64, f64)]) -> (f64, f64) {
    let mut grid_x = 0_i64;
    let mut grid_y = 0_i64;
    let mut direction_x = 1_i64;
    let mut direction_y = 0_i64;
    let mut segment_length = 1_usize;
    let mut segment_steps = 0_usize;
    let mut completed_segments = 0_usize;

    loop {
        let candidate = (
            CANVAS_ORIGIN_X + grid_x as f64 * CANVAS_HORIZONTAL_STEP,
            CANVAS_ORIGIN_Y + grid_y as f64 * CANVAS_VERTICAL_STEP,
        );
        let is_clear = occupied.iter().all(|position| {
            (position.0 - candidate.0).abs() >= CANVAS_HORIZONTAL_STEP
                || (position.1 - candidate.1).abs() >= CANVAS_VERTICAL_STEP
        });
        if is_clear {
            return candidate;
        }

        grid_x += direction_x;
        grid_y += direction_y;
        segment_steps += 1;
        if segment_steps == segment_length {
            segment_steps = 0;
            let previous_x = direction_x;
            direction_x = -direction_y;
            direction_y = previous_x;
            completed_segments += 1;
            if completed_segments.is_multiple_of(2) {
                segment_length += 1;
            }
        }
    }
}

async fn active_canvas_positions(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<Vec<(f64, f64)>, StoreError> {
    Ok(sqlx::query_as(
        "SELECT canvas_nodes.position_x, canvas_nodes.position_y
         FROM canvas_nodes
         JOIN activity_events ON activity_events.id = canvas_nodes.activity_event_id
         WHERE canvas_nodes.deleted_at_us IS NULL
           AND activity_events.deleted_at_us IS NULL",
    )
    .fetch_all(&mut **transaction)
    .await?)
}

async fn ensure_activity_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<(), StoreError> {
    let activity_exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_events WHERE id = ? AND deleted_at_us IS NULL",
    )
    .bind(activity_event_id)
    .fetch_one(&mut **transaction)
    .await?;
    if activity_exists == 0 {
        return Err(StoreError::ActivityNotFound(activity_event_id));
    }
    Ok(())
}

async fn insert_canvas_node_at(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    position_x: f64,
    position_y: f64,
) -> Result<i64, StoreError> {
    let result = sqlx::query(
        "INSERT INTO canvas_nodes (activity_event_id, position_x, position_y) VALUES (?, ?, ?)",
    )
    .bind(activity_event_id)
    .bind(position_x)
    .bind(position_y)
    .execute(&mut **transaction)
    .await?;
    Ok(result.last_insert_rowid())
}

pub(crate) async fn create_canvas_node_in(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<i64, StoreError> {
    ensure_activity_exists(transaction, activity_event_id).await?;
    let occupied = active_canvas_positions(transaction).await?;
    let (position_x, position_y) = next_compact_canvas_position(&occupied);
    let node_id =
        insert_canvas_node_at(transaction, activity_event_id, position_x, position_y).await?;
    bump_canvas_revision(transaction).await?;
    Ok(node_id)
}

impl ActivityStore {
    pub async fn canvas_nodes(&self) -> Result<Vec<CanvasNodeSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, i64, f64, f64)>(
            "SELECT canvas_nodes.id, canvas_nodes.activity_event_id,
                    canvas_nodes.position_x, canvas_nodes.position_y
             FROM canvas_nodes
             JOIN activity_events ON activity_events.id = canvas_nodes.activity_event_id
             WHERE canvas_nodes.deleted_at_us IS NULL
               AND activity_events.deleted_at_us IS NULL
             ORDER BY canvas_nodes.id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(
            |(id, activity_event_id, position_x, position_y)| CanvasNodeSummary {
                id,
                activity_event_id,
                position_x,
                position_y,
            },
        )
        .collect())
    }

    pub async fn create_canvas_node(&self, activity_event_id: i64) -> Result<i64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let node_id = create_canvas_node_in(&mut transaction, activity_event_id).await?;
        transaction.commit().await?;
        Ok(node_id)
    }

    pub async fn create_canvas_node_at(
        &self,
        activity_event_id: i64,
        position_x: f64,
        position_y: f64,
    ) -> Result<i64, StoreError> {
        let mut transaction = self.pool.begin().await?;
        ensure_activity_exists(&mut transaction, activity_event_id).await?;
        let node_id =
            insert_canvas_node_at(&mut transaction, activity_event_id, position_x, position_y)
                .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(node_id)
    }

    pub async fn canvas_position(
        &self,
        canvas_node_id: i64,
    ) -> Result<Option<(f64, f64)>, StoreError> {
        Ok(sqlx::query_as(
            "SELECT position_x, position_y
                 FROM canvas_nodes
                 WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(canvas_node_id)
        .fetch_optional(&self.pool)
        .await?)
    }

    pub async fn update_canvas_position(
        &self,
        canvas_node_id: i64,
        position_x: f64,
        position_y: f64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE canvas_nodes
             SET position_x = ?, position_y = ?
             WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(position_x)
        .bind(position_y)
        .bind(canvas_node_id)
        .execute(&mut *transaction)
        .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_canvas_node(&self, canvas_node_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        let activity_id = sqlx::query_scalar::<_, i64>(
            "SELECT canvas_nodes.activity_event_id
             FROM canvas_nodes
             JOIN activity_events ON activity_events.id = canvas_nodes.activity_event_id
             WHERE canvas_nodes.id = ?
               AND canvas_nodes.deleted_at_us IS NULL
               AND activity_events.deleted_at_us IS NULL",
        )
        .bind(canvas_node_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::CanvasNodeNotFound(canvas_node_id))?;
        soft_delete_activity_in(&mut transaction, activity_id).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn clear_canvas(&self) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM canvas_edges")
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE canvas_nodes
             SET deleted_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             WHERE deleted_at_us IS NULL",
        )
        .execute(&mut *transaction)
        .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn canvas_node_exists(&self, canvas_node_id: i64) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM canvas_nodes
             JOIN activity_events ON activity_events.id = canvas_nodes.activity_event_id
             WHERE canvas_nodes.id = ?
               AND canvas_nodes.deleted_at_us IS NULL
               AND activity_events.deleted_at_us IS NULL",
        )
        .bind(canvas_node_id)
        .fetch_one(&self.pool)
        .await?
            != 0)
    }

    pub async fn create_canvas_edge(
        &self,
        source_node_id: i64,
        target_node_id: i64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("INSERT INTO canvas_edges (source_node_id, target_node_id) VALUES (?, ?)")
            .bind(source_node_id)
            .bind(target_node_id)
            .execute(&mut *transaction)
            .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_canvas_edge(&self, canvas_edge_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::query("DELETE FROM canvas_edges WHERE id = ?")
            .bind(canvas_edge_id)
            .execute(&mut *transaction)
            .await?;
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn canvas_edge_count(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM canvas_edges")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn canvas_edges(&self) -> Result<Vec<CanvasEdgeSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT canvas_edges.id, source_node_id, target_node_id
             FROM canvas_edges
             JOIN canvas_nodes AS source ON source.id = source_node_id
             JOIN canvas_nodes AS target ON target.id = target_node_id
             WHERE source.deleted_at_us IS NULL AND target.deleted_at_us IS NULL
             ORDER BY canvas_edges.id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(id, source_node_id, target_node_id)| CanvasEdgeSummary {
            id,
            source_node_id,
            target_node_id,
        })
        .collect())
    }

    pub async fn canvas_revision(&self) -> Result<i64, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT revision FROM canvas_state_revision WHERE singleton = 1")
                .fetch_one(&self.pool)
                .await?,
        )
    }
}

pub(crate) async fn bump_canvas_revision(
    transaction: &mut Transaction<'_, Sqlite>,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE canvas_state_revision
         SET revision = revision + 1
         WHERE singleton = 1",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
