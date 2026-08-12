#[allow(dead_code)]
#[path = "support/origin_transition.rs"]
mod support;

use support::{harness, record, working_directory};

#[tokio::test]
async fn deleting_a_canvas_node_sets_deleted_at_and_keeps_activity_history() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "soft-delete");
    let first_activity = record(&store, &cwd, "session", "first", 1).await;
    let second_activity = record(&store, &cwd, "session", "second", 2).await;
    let nodes = store.canvas_nodes().await.expect("nodes");
    let first_node = nodes
        .iter()
        .find(|node| node.activity_event_id == first_activity)
        .expect("first node")
        .id;
    let second_node = nodes
        .iter()
        .find(|node| node.activity_event_id == second_activity)
        .expect("second node")
        .id;
    store
        .create_canvas_edge(first_node, second_node)
        .await
        .expect("edge");

    store
        .delete_canvas_node(first_node)
        .await
        .expect("soft delete");

    let deleted_at_us: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at_us FROM canvas_nodes WHERE id = ?")
            .bind(first_node)
            .fetch_one(&pool)
            .await
            .expect("deleted_at_us");
    assert!(deleted_at_us.is_some());
    assert_eq!(
        store
            .canvas_nodes()
            .await
            .expect("active nodes")
            .iter()
            .map(|node| node.id)
            .collect::<Vec<_>>(),
        vec![second_node]
    );
    assert!(!store.canvas_node_exists(first_node).await.expect("exists"));
    assert_eq!(
        store.canvas_position(first_node).await.expect("position"),
        None
    );
    assert!(store.canvas_edges().await.expect("edges").is_empty());
    assert!(
        !store
            .activity_detail(first_activity)
            .await
            .expect("detail")
            .on_canvas
    );
    assert_eq!(store.activity_count().await.expect("activities"), 2);
}

#[tokio::test]
async fn clearing_the_canvas_soft_deletes_every_active_node() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "soft-clear");
    record(&store, &cwd, "session", "first", 1).await;
    record(&store, &cwd, "session", "second", 2).await;

    store.clear_canvas().await.expect("clear canvas");

    assert!(store.canvas_nodes().await.expect("active nodes").is_empty());
    let preserved: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM canvas_nodes WHERE deleted_at_us IS NOT NULL")
            .fetch_one(&pool)
            .await
            .expect("preserved nodes");
    assert_eq!(preserved, 2);
    assert_eq!(store.activity_count().await.expect("activities"), 2);
}
