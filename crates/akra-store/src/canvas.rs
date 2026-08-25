use std::collections::HashSet;

use sqlx::{Sqlite, Transaction};

use crate::{
    ActivityStore, CanvasEdgeSummary, CanvasNodeSummary, StoreError,
    work_curation::soft_delete_activity_in,
};

pub(crate) const CANVAS_ORIGIN_X: f64 = 64.0;
pub(crate) const CANVAS_ORIGIN_Y: f64 = 64.0;
const CANVAS_HORIZONTAL_STEP: f64 = 336.0;
const CANVAS_VERTICAL_STEP: f64 = 220.0;

pub(crate) struct CompactCanvasAllocator {
    blocked: HashSet<(i64, i64)>,
    grid_x: i64,
    grid_y: i64,
    direction_x: i64,
    direction_y: i64,
    segment_length: usize,
    segment_steps: usize,
    completed_segments: usize,
}

impl CompactCanvasAllocator {
    pub(crate) fn new(occupied: &[(f64, f64)]) -> Self {
        let mut allocator = Self {
            blocked: HashSet::with_capacity(occupied.len().saturating_mul(4)),
            grid_x: 0,
            grid_y: 0,
            direction_x: 1,
            direction_y: 0,
            segment_length: 1,
            segment_steps: 0,
            completed_segments: 0,
        };
        for &(position_x, position_y) in occupied {
            allocator.block_position(position_x, position_y);
        }
        allocator
    }

    fn block_position(&mut self, position_x: f64, position_y: f64) {
        let Some(x_indices) =
            blocking_grid_indices(position_x, CANVAS_ORIGIN_X, CANVAS_HORIZONTAL_STEP)
        else {
            return;
        };
        let Some(y_indices) =
            blocking_grid_indices(position_y, CANVAS_ORIGIN_Y, CANVAS_VERTICAL_STEP)
        else {
            return;
        };
        for grid_x in x_indices.into_iter().flatten() {
            for grid_y in y_indices.into_iter().flatten() {
                self.blocked.insert((grid_x, grid_y));
            }
        }
    }

    pub(crate) fn next_position(&mut self) -> (f64, f64) {
        loop {
            let candidate_grid = (self.grid_x, self.grid_y);
            self.advance();
            if self.blocked.insert(candidate_grid) {
                return (
                    CANVAS_ORIGIN_X + candidate_grid.0 as f64 * CANVAS_HORIZONTAL_STEP,
                    CANVAS_ORIGIN_Y + candidate_grid.1 as f64 * CANVAS_VERTICAL_STEP,
                );
            }
        }
    }

    fn advance(&mut self) {
        self.grid_x += self.direction_x;
        self.grid_y += self.direction_y;
        self.segment_steps += 1;
        if self.segment_steps == self.segment_length {
            self.segment_steps = 0;
            let previous_x = self.direction_x;
            self.direction_x = -self.direction_y;
            self.direction_y = previous_x;
            self.completed_segments += 1;
            if self.completed_segments.is_multiple_of(2) {
                self.segment_length += 1;
            }
        }
    }
}

fn blocking_grid_indices(position: f64, origin: f64, step: f64) -> Option<[Option<i64>; 2]> {
    if !position.is_finite() {
        return None;
    }
    let grid_position = (position - origin) / step;
    if grid_position < i64::MIN as f64 || grid_position > i64::MAX as f64 {
        return None;
    }
    let lower = grid_position.floor() as i64;
    let upper = grid_position.ceil() as i64;
    Some([Some(lower), (upper != lower).then_some(upper)])
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
    let (position_x, position_y) = CompactCanvasAllocator::new(&occupied).next_position();
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

#[cfg(test)]
mod tests {
    use super::CompactCanvasAllocator;

    #[test]
    fn compact_allocator_assigns_large_layout_in_one_spiral_pass() {
        let mut allocator = CompactCanvasAllocator::new(&[]);
        let positions = (0..10_000)
            .map(|_| {
                let (position_x, position_y) = allocator.next_position();
                (position_x.to_bits(), position_y.to_bits())
            })
            .collect::<std::collections::HashSet<_>>();

        assert_eq!(positions.len(), 10_000);
        assert!(positions.contains(&(64.0_f64.to_bits(), 64.0_f64.to_bits())));
        assert!(positions.contains(&(400.0_f64.to_bits(), 64.0_f64.to_bits())));
        assert!(positions.contains(&(400.0_f64.to_bits(), 284.0_f64.to_bits())));
    }
}
