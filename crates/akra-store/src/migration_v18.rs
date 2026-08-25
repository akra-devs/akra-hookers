use sqlx::{Sqlite, Transaction};

use crate::canvas::{CANVAS_ORIGIN_X, CANVAS_ORIGIN_Y, CompactCanvasAllocator};

/// Spreads untouched legacy canvas nodes that all inherited the same origin.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 18")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    let rows = sqlx::query_as::<_, (i64, f64, f64)>(
        "SELECT canvas_nodes.id, canvas_nodes.position_x, canvas_nodes.position_y
         FROM canvas_nodes
         JOIN activity_events ON activity_events.id = canvas_nodes.activity_event_id
         WHERE canvas_nodes.deleted_at_us IS NULL
           AND activity_events.deleted_at_us IS NULL
         ORDER BY activity_events.global_sequence IS NULL,
                  activity_events.global_sequence,
                  activity_events.id,
                  canvas_nodes.id",
    )
    .fetch_all(&mut **transaction)
    .await?;

    let mut legacy_node_ids = Vec::new();
    let mut occupied = Vec::new();
    for (node_id, position_x, position_y) in rows {
        if position_x == CANVAS_ORIGIN_X && position_y == CANVAS_ORIGIN_Y {
            legacy_node_ids.push(node_id);
        } else {
            occupied.push((position_x, position_y));
        }
    }

    if !legacy_node_ids.is_empty() {
        occupied.push((CANVAS_ORIGIN_X, CANVAS_ORIGIN_Y));
    }
    let mut allocator = CompactCanvasAllocator::new(&occupied);
    let mut moved = 0_usize;
    for node_id in legacy_node_ids.into_iter().skip(1) {
        let (position_x, position_y) = allocator.next_position();
        sqlx::query("UPDATE canvas_nodes SET position_x = ?, position_y = ? WHERE id = ?")
            .bind(position_x)
            .bind(position_y)
            .bind(node_id)
            .execute(&mut **transaction)
            .await?;
        moved += 1;
    }

    if moved != 0 {
        sqlx::query("UPDATE canvas_state_revision SET revision = revision + 1 WHERE singleton = 1")
            .execute(&mut **transaction)
            .await?;
    }
    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (18, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use akra_core::ingress::IngressEvent;
    use akra_git::ProjectIdentity;

    use crate::{ActivityStore, RecordActivity};

    #[tokio::test]
    async fn migration_spreads_only_untouched_legacy_nodes_and_is_idempotent() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("cwd");
        for index in 0..4 {
            let event = IngressEvent::try_new(
                "codex",
                "legacy-layout-session",
                format!("turn-{index}"),
                cwd.to_string_lossy(),
                format!("prompt {index}"),
                None,
            )
            .expect("event");
            let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
                .expect("origin")
                .origin;
            store
                .record(RecordActivity::captured(event, origin, index + 1))
                .await
                .expect("activity");
        }

        let nodes = store.canvas_nodes().await.expect("nodes");
        let manually_positioned = nodes[3].id;
        sqlx::query("UPDATE canvas_nodes SET position_x = 64, position_y = 64")
            .execute(&store.pool)
            .await
            .expect("legacy pile");
        sqlx::query("UPDATE canvas_nodes SET position_x = 1200, position_y = 720 WHERE id = ?")
            .bind(manually_positioned)
            .execute(&store.pool)
            .await
            .expect("manual position");
        sqlx::query("DELETE FROM schema_migrations WHERE version = 18")
            .execute(&store.pool)
            .await
            .expect("rewind marker");
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM canvas_state_revision WHERE singleton = 1")
                .fetch_one(&store.pool)
                .await
                .expect("revision");

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction).await.expect("migration");
        transaction.commit().await.expect("commit");

        let positions = store
            .canvas_nodes()
            .await
            .expect("migrated nodes")
            .into_iter()
            .map(|node| (node.id, node.position_x, node.position_y))
            .collect::<Vec<_>>();
        assert_eq!(
            positions
                .iter()
                .find(|(id, _, _)| *id == manually_positioned)
                .map(|(_, x, y)| (*x, *y)),
            Some((1_200.0, 720.0))
        );
        let distinct = positions
            .iter()
            .map(|(_, x, y)| (x.to_bits(), y.to_bits()))
            .collect::<std::collections::BTreeSet<_>>();
        assert_eq!(distinct.len(), positions.len());
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM canvas_state_revision WHERE singleton = 1",
            )
            .fetch_one(&store.pool)
            .await
            .expect("new revision"),
            revision + 1
        );

        let mut repeat = store.pool.begin().await.expect("repeat transaction");
        super::apply(&mut repeat)
            .await
            .expect("idempotent migration");
        repeat.commit().await.expect("repeat commit");
        assert_eq!(
            store
                .canvas_nodes()
                .await
                .expect("stable nodes")
                .into_iter()
                .map(|node| (node.id, node.position_x, node.position_y))
                .collect::<Vec<_>>(),
            positions
        );
    }
}
