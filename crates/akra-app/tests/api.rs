use std::sync::Arc;

use akra_app::http::app;
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    app("fixture-token", store)
}

#[tokio::test]
async fn rejects_ingest_without_bearer_capability() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ingest")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"prompt":"hello"}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
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
                .uri("/v1/activities")
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
async fn deletes_canvas_node_without_deleting_activity() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = store
        .record("codex", "s", "t", "C:\\x", "keep")
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
    assert_eq!(store.activity_count().await.expect("activity remains"), 1);
}

#[tokio::test]
async fn updates_canvas_position_without_mutating_activity() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = store
        .record("codex", "s", "turn", "C:\\x", "keep")
        .await
        .expect("activity");
    let node_id = store.create_canvas_node(activity_id).await.expect("node");

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
    let first = store
        .record("codex", "s", "a", "C:\\x", "first")
        .await
        .expect("first");
    let second = store
        .record("codex", "s", "b", "C:\\x", "second")
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
async fn provider_toggle_changes_future_capture_without_deleting_history() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    store
        .record("codex", "s", "history", "C:\\x", "keep")
        .await
        .expect("history");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/v1/providers/codex")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":false}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("history remains"), 1);
    assert!(
        !store
            .provider_enabled("codex")
            .await
            .expect("provider status")
    );
}
