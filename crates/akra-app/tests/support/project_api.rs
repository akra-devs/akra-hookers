use axum::{Router, http::StatusCode};
use serde_json::{Value, json};

use crate::api_harness::call;

pub(crate) async fn merge(app: &Router, source: i64, target: i64) -> (StatusCode, Value) {
    call(
        app,
        axum::http::Method::POST,
        &format!("/v1/projects/{source}/merge"),
        Some(json!({"target_project_id": target})),
        true,
    )
    .await
}

pub(crate) async fn ingest(app: &Router, session: &str, turn: &str, cwd: &str, prompt: &str) {
    assert_eq!(
        call(
            app,
            axum::http::Method::POST,
            "/v1/ingest",
            Some(json!({"session_id": session, "turn_id": turn, "cwd": cwd, "prompt": prompt})),
            true,
        )
        .await
        .0,
        StatusCode::ACCEPTED
    );
}

pub(crate) fn project_id(projects: &Value, name: &str) -> i64 {
    projects
        .as_array()
        .expect("projects")
        .iter()
        .find(|project| project["name"] == name)
        .and_then(|project| project["id"].as_i64())
        .expect("project id")
}
