use super::*;

#[tokio::test]
async fn updates_canvas_position_without_mutating_activity() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = record(&store, "codex", "s", "turn", "C:\\x", "keep")
        .await
        .expect("activity");
    let node_id = store.create_canvas_node(activity_id).await.expect("node");
    let revision = store.canvas_revision().await.expect("revision");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/canvas/{node_id}"))
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"position_x":200,"position_y":300}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(store.canvas_revision().await.expect("revision") > revision);
    assert_eq!(
        store.canvas_position(node_id).await.expect("position"),
        Some((200.0, 300.0))
    );
    assert_eq!(store.activity_count().await.expect("activity remains"), 1);
}

#[tokio::test]
async fn creates_canvas_edge_without_mutating_activities() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let first = record(&store, "codex", "s", "a", "C:\\x", "first")
        .await
        .expect("first");
    let second = record(&store, "codex", "s", "b", "C:\\x", "second")
        .await
        .expect("second");
    let source = store.create_canvas_node(first).await.expect("source");
    let target = store.create_canvas_node(second).await.expect("target");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/canvas/edges")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(format!(
                    r#"{{"source_node_id":{source},"target_node_id":{target}}}"#
                )))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(store.canvas_edge_count().await.expect("edge"), 1);
    assert_eq!(store.activity_count().await.expect("activities"), 2);
}

#[tokio::test]
async fn deletes_canvas_edge_without_mutating_nodes_or_activities() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let first = record(&store, "codex", "s", "a", "C:\\x", "first")
        .await
        .expect("first");
    let second = record(&store, "codex", "s", "b", "C:\\x", "second")
        .await
        .expect("second");
    let source = store.create_canvas_node(first).await.expect("source");
    let target = store.create_canvas_node(second).await.expect("target");
    store
        .create_canvas_edge(source, target)
        .await
        .expect("edge");
    let edge_id = store.canvas_edges().await.expect("edges")[0].id;
    let node_count = store.canvas_nodes().await.expect("nodes").len();
    let activity_count = store.activity_count().await.expect("activities");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/canvas/edges/{edge_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(store.canvas_edges().await.expect("edges").is_empty());
    assert_eq!(store.canvas_nodes().await.expect("nodes").len(), node_count);
    assert_eq!(
        store.activity_count().await.expect("activities"),
        activity_count
    );
}
