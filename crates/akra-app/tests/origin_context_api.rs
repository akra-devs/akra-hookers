use std::path::{Path, PathBuf};

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

#[path = "support/api_harness.rs"]
mod api_harness;
#[path = "support/origin_api.rs"]
mod origin_api;
use api_harness::{call, create_project as create, harness};
use origin_api::record_with_origin;

#[tokio::test]
async fn origins_list_full_paths_counts_and_recommend_root_home_without_forcing() {
    let harness = harness().await;
    let root = PathBuf::from(r"C:\");
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .expect("Windows home");
    let other = PathBuf::from(r"C:\dev\other-work");
    record_with_origin(&harness.store, "root", "one", &root, "directory:root").await;
    record_with_origin(&harness.store, "home", "one", &home, "directory:home").await;
    record_with_origin(&harness.store, "other", "one", &other, "directory:other").await;

    let (status, origins) = call(&harness.app, Method::GET, "/v1/origins", None, true).await;

    assert_eq!(status, StatusCode::OK);
    assert_eq!(origins.as_array().expect("origins").len(), 3);
    assert_origin(&origins, &root, "shared");
    assert_origin(&origins, &home, "shared");
    let other_origin = assert_origin(&origins, &other, "dedicated");
    let project_id = other_origin["default_project_id"]
        .as_i64()
        .expect("default project");
    let (status, project_origins) = call(
        &harness.app,
        Method::GET,
        &format!("/v1/projects/{project_id}/origins"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        project_origins.as_array().expect("project origins").len(),
        1
    );
    assert_eq!(
        project_origins[0]["display_path"].as_str(),
        Some(other.to_string_lossy().as_ref())
    );
    assert_eq!(
        call(
            &harness.app,
            Method::GET,
            "/v1/projects/999999/origins",
            None,
            true,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn origins_routing_requires_confirmation_and_maps_transition_errors() {
    let harness = harness().await;
    let cwd = PathBuf::from(r"C:\dev\setup");
    record_with_origin(&harness.store, "session", "one", &cwd, "directory:setup").await;
    let (_, origins) = call(&harness.app, Method::GET, "/v1/origins", None, true).await;
    let origin_id = origins[0]["id"].as_i64().expect("origin id");
    let current_project = origins[0]["default_project_id"]
        .as_i64()
        .expect("project id");

    let confirmed = patch(
        &harness.app,
        origin_id,
        json!({"mode": "dedicated", "destination": {"project_id": current_project}, "confirm": true}),
    )
    .await;
    assert_eq!(confirmed.0, StatusCode::OK);
    assert_eq!(confirmed.1["setup_state"], "confirmed");
    assert_eq!(
        patch(
            &harness.app,
            origin_id,
            json!({"mode": "dedicated", "destination": {"project_id": current_project}, "confirm": true}),
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, target) = create(&harness.app, "Connected").await;
    let target_id = target["id"].as_i64().expect("target");
    let moved = patch(
        &harness.app,
        origin_id,
        json!({"mode": "dedicated", "destination": {"project_id": target_id}, "confirm": true}),
    )
    .await;
    assert_eq!(moved.0, StatusCode::OK);
    assert_eq!(moved.1["default_project_id"], target_id);
    let shared = patch(
        &harness.app,
        origin_id,
        json!({"mode": "shared", "confirm": true}),
    )
    .await;
    assert_eq!(shared.0, StatusCode::OK);
    assert_eq!(shared.1["routing_mode"], "shared");
    assert_eq!(shared.1["default_project_id"], Value::Null);

    for payload in [
        json!({"mode": "shared"}),
        json!({"mode": "shared", "destination": {"project_id": target_id}, "confirm": true}),
        json!({"mode": "dedicated", "confirm": true}),
        json!({"mode": "unknown", "confirm": true}),
    ] {
        assert_eq!(
            patch(&harness.app, origin_id, payload).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (_, routing_error) = patch(
        &harness.app,
        origin_id,
        json!({
            "mode": "shared",
            "destination": {"project_id": target_id},
            "confirm": true
        }),
    )
    .await;
    assert_eq!(routing_error["code"], "invalid_origin_routing");
    assert!(routing_error["message"].is_string());
    assert_eq!(
        patch(
            &harness.app,
            999999,
            json!({"mode": "shared", "confirm": true}),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        patch(
            &harness.app,
            origin_id,
            json!({"mode": "dedicated", "destination": {"project_id": 999999}, "confirm": true}),
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn origins_routes_require_bearer_authentication() {
    let harness = harness().await;
    for (method, uri, body) in [
        (Method::GET, "/v1/origins", None),
        (Method::GET, "/v1/projects/1/origins", None),
        (
            Method::PATCH,
            "/v1/origins/1/routing",
            Some(json!({"mode": "shared", "confirm": true})),
        ),
    ] {
        assert_eq!(
            call(&harness.app, method, uri, body, false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}

fn assert_origin<'a>(origins: &'a Value, path: &Path, recommendation: &str) -> &'a Value {
    let origin = origins
        .as_array()
        .expect("origins")
        .iter()
        .find(|origin| origin["display_path"] == path.to_string_lossy().as_ref())
        .expect("origin");
    assert_eq!(origin["kind"], "directory");
    assert_eq!(origin["resolution_source"], "captured");
    assert_eq!(origin["setup_state"], "unconfirmed");
    assert_eq!(origin["routing_mode"], "dedicated");
    assert_eq!(origin["recommended_mode"], recommendation);
    assert_eq!(origin["activity_count"], 1);
    assert_eq!(origin["conversation_count"], 1);
    origin
}

async fn patch(app: &axum::Router, origin_id: i64, body: Value) -> (StatusCode, Value) {
    call(
        app,
        Method::PATCH,
        &format!("/v1/origins/{origin_id}/routing"),
        Some(body),
        true,
    )
    .await
}
