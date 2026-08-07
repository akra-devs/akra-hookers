use akra_store::ActivityStore;

#[tokio::test]
async fn duplicate_ingress_creates_one_immutable_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");

    let first = store
        .record("codex", "session-1", "turn-1", "C:\\project", "prompt")
        .await
        .expect("first event records");
    let duplicate = store
        .record("codex", "session-1", "turn-1", "C:\\project", "prompt")
        .await
        .expect("duplicate is accepted");

    assert_eq!(first, duplicate);
    assert_eq!(store.activity_count().await.expect("count succeeds"), 1);
}

#[tokio::test]
async fn deleting_canvas_state_does_not_delete_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let activity_id = store
        .record("codex", "session-2", "turn-2", "C:\\project", "keep this")
        .await
        .expect("event records");

    let canvas_node_id = store
        .create_canvas_node(activity_id)
        .await
        .expect("canvas node records");
    store
        .delete_canvas_node(canvas_node_id)
        .await
        .expect("canvas node deletes");

    assert_eq!(store.activity_count().await.expect("count succeeds"), 1);
}

#[tokio::test]
async fn canvas_positions_persist_independently_of_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let activity_id = store
        .record("codex", "session-3", "turn-3", "C:\\project", "position me")
        .await
        .expect("event records");

    let node_id = store
        .create_canvas_node_at(activity_id, 120.0, 220.0)
        .await
        .expect("node records");

    assert_eq!(
        store
            .canvas_position(node_id)
            .await
            .expect("position loads"),
        Some((120.0, 220.0))
    );
    assert_eq!(store.activity_count().await.expect("activity remains"), 1);
}

#[tokio::test]
async fn canvas_edges_persist_independently_of_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let first = store
        .record("codex", "session-4", "turn-4", "C:\\project", "first")
        .await
        .expect("first activity");
    let second = store
        .record("codex", "session-4", "turn-5", "C:\\project", "second")
        .await
        .expect("second activity");
    let source = store.create_canvas_node(first).await.expect("source node");
    let target = store.create_canvas_node(second).await.expect("target node");

    store
        .create_canvas_edge(source, target)
        .await
        .expect("edge records");
    assert_eq!(store.canvas_edge_count().await.expect("edge count"), 1);
    assert_eq!(store.activity_count().await.expect("activities remain"), 2);
}
