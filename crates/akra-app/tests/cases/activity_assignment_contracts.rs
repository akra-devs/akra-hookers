use std::path::Path;

use axum::http::{Method, StatusCode};
use serde_json::{Value, json};

use crate::{
    api_harness::{call, create_project, harness},
    origin_api::record_with_origin,
};

#[tokio::test]
async fn assignments_selection_route_replace_clear_and_inbox_are_stable() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\shared");
    let first = record_with_origin(&harness.store, "long", "one", cwd, "directory:shared").await;
    let second = record_with_origin(&harness.store, "long", "two", cwd, "directory:shared").await;
    let third = record_with_origin(&harness.store, "long", "three", cwd, "directory:shared").await;
    let origin_id = harness.store.origins().await.expect("origins")[0].id;
    harness
        .store
        .configure_origin(origin_id, akra_store::OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    assert_eq!(
        assign(
            &harness.app,
            json!({"activity_ids": [first, second, third], "destination": null}),
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, project_a) = create_project(&harness.app, "A").await;
    let project_a = project_a["id"].as_i64().expect("A");
    let assigned = assign(
        &harness.app,
        json!({
            "activity_ids": [third, first, second, first],
            "destination": {"project_id": project_a}
        }),
    )
    .await;
    assert_eq!(assigned.0, StatusCode::OK);
    assert_eq!(assigned.1["activity_ids"], json!([first, second, third]));
    assert_eq!(assigned.1["project_id"], project_a);
    assert_eq!(assigned.1["future_route"], "unchanged");
    assert_eq!(
        assign(
            &harness.app,
            json!({
                "activity_ids": [third],
                "destination": {"project_id": project_a},
                "future_route": "set"
            }),
        )
        .await
        .0,
        StatusCode::OK
    );
    let fourth = record_with_origin(&harness.store, "long", "four", cwd, "directory:shared").await;
    let fifth = record_with_origin(&harness.store, "long", "five", cwd, "directory:shared").await;
    let (_, project_b) = create_project(&harness.app, "B").await;
    let project_b = project_b["id"].as_i64().expect("B");
    assert_eq!(
        assign(
            &harness.app,
            json!({
                "activity_ids": [fifth],
                "destination": {"project_id": project_b},
                "future_route": "set"
            }),
        )
        .await
        .0,
        StatusCode::OK
    );
    let sixth = record_with_origin(&harness.store, "long", "six", cwd, "directory:shared").await;
    assert_eq!(
        assign(
            &harness.app,
            json!({
                "activity_ids": [sixth],
                "destination": {"project_id": project_b},
                "future_route": "clear"
            }),
        )
        .await
        .0,
        StatusCode::OK
    );
    let seventh =
        record_with_origin(&harness.store, "long", "seven", cwd, "directory:shared").await;
    let projects = harness.store.projects().await.expect("projects");
    assert_eq!(project_count(&projects, project_a), 4);
    assert_eq!(project_count(&projects, project_b), 2);
    assert_eq!(harness.store.activity_count().await.expect("count"), 7);
    assert!(fourth < fifth && fifth < sixth && sixth < seventh);
}

#[tokio::test]
async fn assignments_invalid_mixed_requests_are_atomic_and_authenticated() {
    let harness = harness().await;
    let shared = Path::new(r"C:\shared");
    let dedicated = Path::new(r"C:\dedicated");
    let shared_a = record_with_origin(&harness.store, "a", "one", shared, "directory:shared").await;
    let shared_b = record_with_origin(&harness.store, "b", "one", shared, "directory:shared").await;
    let dedicated_id =
        record_with_origin(&harness.store, "a", "two", dedicated, "directory:dedicated").await;
    let shared_origin = harness
        .store
        .origins()
        .await
        .expect("origins")
        .into_iter()
        .find(|origin| origin.display_path == shared.to_string_lossy())
        .expect("shared origin")
        .id;
    harness
        .store
        .configure_origin(
            shared_origin,
            akra_store::OriginRoutingCommand::shared(true),
        )
        .await
        .expect("shared");
    let (_, target) = create_project(&harness.app, "Target").await;
    let target = target["id"].as_i64().expect("target");

    for (case, body, expected) in [
        (
            "empty",
            json!({"activity_ids": [], "destination": {"project_id": target}}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "omitted destination",
            json!({"activity_ids": [shared_a]}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "mixed mode",
            json!({"activity_ids": [shared_a, dedicated_id], "destination": {"project_id": target}}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "mixed conversation route",
            json!({"activity_ids": [shared_a, shared_b], "destination": {"project_id": target}, "future_route": "set"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "route without destination",
            json!({"activity_ids": [shared_a], "destination": null, "future_route": "set"}),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "unknown activity",
            json!({"activity_ids": [999999], "destination": {"project_id": target}}),
            StatusCode::NOT_FOUND,
        ),
        (
            "unknown project",
            json!({"activity_ids": [shared_a], "destination": {"project_id": 999999}}),
            StatusCode::NOT_FOUND,
        ),
        (
            "oversized batch",
            json!({
                "activity_ids": (1..=101).collect::<Vec<_>>(),
                "destination": {"project_id": target}
            }),
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
    ] {
        let (status, error) = assign(&harness.app, body).await;
        assert_eq!(status, expected, "{case}");
        assert!(error["code"].is_string(), "{case}: {error}");
        assert!(error["message"].is_string(), "{case}: {error}");
        assert_eq!(project_activity_count(&harness.store, target).await, 0);
    }
    assert_eq!(
        call(
            &harness.app,
            Method::POST,
            "/v1/activity-assignments",
            Some(json!({"activity_ids": [shared_a], "destination": null})),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn assign(app: &axum::Router, body: Value) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/v1/activity-assignments",
        Some(body),
        true,
    )
    .await
}

fn project_count(projects: &[akra_store::ProjectSummary], project_id: i64) -> i64 {
    projects
        .iter()
        .find(|project| project.id == project_id)
        .map(|project| project.activity_count)
        .expect("project")
}

async fn project_activity_count(store: &akra_store::ActivityStore, project_id: i64) -> i64 {
    project_count(&store.projects().await.expect("projects"), project_id)
}
