use super::*;

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

    let project_id = store
        .projects()
        .await
        .expect("projects")
        .into_iter()
        .find(|project| project.name == "project-one")
        .expect("project one")
        .id;
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/activities?scope=project&project_id={project_id}"
                ))
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
                .uri("/v1/activities?scope=all")
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
