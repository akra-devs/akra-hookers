use super::*;

#[tokio::test]
async fn clearing_canvas_preserves_immutable_activities() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = record(
        &store,
        "codex",
        "session",
        "turn",
        "project",
        "keep immutable",
    )
    .await
    .expect("activity");
    store
        .create_canvas_node(activity_id)
        .await
        .expect("canvas node");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri("/v1/canvas")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("activity count"), 1);
    assert!(
        store.canvas_nodes().await.expect("canvas nodes").is_empty(),
        "canvas-only clear must not recreate or retain nodes"
    );
}

#[tokio::test]
async fn rejects_malformed_ingest_with_valid_bearer() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ingest")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from("{"))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn rejects_structurally_incomplete_ingest() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ingest")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"prompt":"hello"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNPROCESSABLE_ENTITY);
}

#[tokio::test]
async fn accepts_and_persists_a_valid_ingest() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ingest")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"session_id":"s","turn_id":"t","cwd":"C:\\x","prompt":"persist"}"#,
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::ACCEPTED);
    assert_eq!(store.activity_count().await.expect("count"), 1);
}

#[tokio::test]
async fn accepts_duplicate_ingest_without_duplicate_activity() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let payload = r#"{"session_id":"s","turn_id":"t","cwd":"C:\\x","prompt":"persist"}"#;

    for _ in 0..2 {
        let response = app("fixture-token", Arc::clone(&store))
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ingest")
                    .header("authorization", "Bearer fixture-token")
                    .header("content-type", "application/json")
                    .body(Body::from(payload))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }
    assert_eq!(store.activity_count().await.expect("count"), 1);
}

#[tokio::test]
async fn lists_projects_after_ingest() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/projects")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn lists_activities_with_bearer_capability() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/activities?scope=all")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn lists_persisted_canvas_nodes_with_bearer_capability() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .uri("/v1/canvas")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn deletes_canvas_node_and_tombstones_activity() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = record(&store, "codex", "s", "t", "C:\\x", "keep")
        .await
        .expect("activity");
    let node_id = store.create_canvas_node(activity_id).await.expect("node");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/canvas/{node_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("activity deleted"), 0);
    let detail = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .uri(format!("/v1/activities/{activity_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(detail.status(), StatusCode::NOT_FOUND);
}
