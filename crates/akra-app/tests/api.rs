use std::{fs, sync::Arc};

use akra_adapters::codex::CodexHookLifecycle;
use akra_app::http::{app, app_with_codex_lifecycle};
use axum::{
    body::{Body, to_bytes},
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
async fn activity_queries_are_scoped_to_the_requested_project() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let app = app("fixture-token", Arc::clone(&store));

    for (turn_id, cwd, prompt) in [
        ("one", "project-one", "first project"),
        ("two", "project-two", "second project"),
    ] {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/ingest")
                    .header("authorization", "Bearer fixture-token")
                    .header("content-type", "application/json")
                    .body(Body::from(format!(
                        r#"{{"session_id":"session","turn_id":"{turn_id}","cwd":"{cwd}","prompt":"{prompt}"}}"#
                    )))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::ACCEPTED);
    }

    let response = app
        .oneshot(
            Request::builder()
                .uri("/v1/activities?project=project-one")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let activities: Vec<serde_json::Value> = serde_json::from_slice(&body).expect("JSON");

    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0]["prompt"], "first project");
}

#[tokio::test]
async fn permits_local_dashboard_cors_preflight() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("OPTIONS")
                .uri("/v1/activities")
                .header("origin", "http://127.0.0.1:5174")
                .header("access-control-request-method", "GET")
                .header("access-control-request-headers", "authorization")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");

    assert!(response.status().is_success());
    assert_eq!(
        response.headers().get("access-control-allow-origin"),
        Some(&"http://127.0.0.1:5174".parse().expect("header"))
    );
}

#[tokio::test]
async fn rejects_canvas_edges_for_unknown_nodes_as_client_input() {
    let response = test_app()
        .await
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/canvas/edges")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"source_node_id":99,"target_node_id":100}"#))
                .expect("request"),
        )
        .await
        .expect("response");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn clearing_canvas_preserves_immutable_activities() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = store
        .record("codex", "session", "turn", "project", "keep immutable")
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

#[tokio::test]
async fn provider_toggle_synchronizes_the_global_codex_manifest() {
    let home = tempfile::TempDir::new().expect("temporary Codex home");
    let codex_directory = home.path().join(".codex");
    fs::create_dir_all(&codex_directory).expect("Codex directory");
    fs::write(
        codex_directory.join("hooks.json"),
        r#"{
          "hooks": {
            "UserPromptSubmit": [{
              "hooks": [{
                "type": "command",
                "command": "third-party-hook"
              }]
            }]
          }
        }"#,
    )
    .expect("third-party hook");

    let command = r#""C:\tools\akra-hookers.exe" capture --data-dir "C:\data""#;
    let lifecycle = Arc::new(CodexHookLifecycle::new(home.path()));
    lifecycle.enable(command).expect("initial hook");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    store
        .record("codex", "session", "turn", "project", "history survives")
        .await
        .expect("history");
    assert_eq!(store.canvas_nodes().await.expect("canvas").len(), 1);

    let off = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        Arc::clone(&lifecycle),
        command.to_owned(),
    )
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
    assert_eq!(off.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("history"), 1);
    assert_eq!(store.canvas_nodes().await.expect("canvas").len(), 1);

    let after_off: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(codex_directory.join("hooks.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    let after_off_commands = manifest_commands(&after_off);
    assert_eq!(after_off_commands, vec!["third-party-hook"]);

    let on = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        lifecycle,
        command.to_owned(),
    )
    .oneshot(
        Request::builder()
            .method("PATCH")
            .uri("/v1/providers/codex")
            .header("authorization", "Bearer fixture-token")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":true}"#))
            .expect("request"),
    )
    .await
    .expect("response");
    assert_eq!(on.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.canvas_nodes().await.expect("canvas").len(), 1);

    let after_on: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(codex_directory.join("hooks.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    let after_on_commands = manifest_commands(&after_on);
    assert_eq!(
        after_on_commands,
        vec!["third-party-hook", command],
        "ON restores only one asynchronous akra hook"
    );
    let hooks = &after_on["hooks"]["UserPromptSubmit"];
    assert_eq!(hooks[1]["hooks"][0]["async"], true);
}

fn manifest_commands(manifest: &serde_json::Value) -> Vec<&str> {
    manifest["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("prompt submit groups")
        .iter()
        .flat_map(|group| group["hooks"].as_array().expect("hook commands"))
        .map(|hook| hook["command"].as_str().expect("command"))
        .collect()
}
