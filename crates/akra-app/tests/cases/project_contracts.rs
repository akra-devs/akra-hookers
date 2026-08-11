use axum::http::{Method, StatusCode};
use serde_json::json;

use crate::{
    api_harness::{call, create_project as create, harness},
    project_api::{ingest, merge, project_id},
};

#[tokio::test]
async fn projects_create_rename_list_and_reject_normalized_collisions() {
    let harness = harness().await;
    let (created_status, created) = call(
        &harness.app,
        Method::POST,
        "/v1/projects",
        Some(json!({"name": "  아크라 프로젝트  "})),
        true,
    )
    .await;
    assert_eq!(created_status, StatusCode::CREATED);
    let id = created["id"].as_i64().expect("project id");
    assert_eq!(created["name"], "아크라 프로젝트");
    let (renamed_status, renamed) = call(
        &harness.app,
        Method::PATCH,
        &format!("/v1/projects/{id}"),
        Some(json!({"name": "Renamed"})),
        true,
    )
    .await;
    assert_eq!(renamed_status, StatusCode::OK);
    assert_eq!(renamed["id"], id);
    assert_eq!(renamed["name"], "Renamed");

    assert_eq!(create(&harness.app, "zeta").await.0, StatusCode::CREATED);
    assert_eq!(create(&harness.app, "Alpha").await.0, StatusCode::CREATED);
    let (collision_status, collision) = create(&harness.app, "ＡＬＰＨＡ").await;
    assert_eq!(collision_status, StatusCode::CONFLICT);
    assert_eq!(collision["code"], "project_name_conflict");
    assert_eq!(
        collision["message"],
        "A project with that name already exists."
    );
    assert_eq!(
        call(
            &harness.app,
            Method::PATCH,
            &format!("/v1/projects/{id}"),
            Some(json!({"name": " alpha "})),
            true,
        )
        .await
        .0,
        StatusCode::CONFLICT
    );

    let (status, listed) = call(&harness.app, Method::GET, "/v1/projects", None, true).await;
    assert_eq!(status, StatusCode::OK);
    let names = listed
        .as_array()
        .expect("project list")
        .iter()
        .map(|project| project["name"].as_str().expect("name"))
        .collect::<Vec<_>>();
    assert_eq!(names, vec!["Alpha", "Renamed", "zeta"]);
    for project in listed.as_array().expect("projects") {
        assert!(project["origin_count"].is_u64());
        assert!(project["activity_count"].is_u64());
        assert!(project["needs_setup"].is_boolean());
        assert!(project.get("latest_activity_at_us").is_some());
    }
}

#[tokio::test]
async fn projects_validate_names_ids_same_target_and_delete_absence() {
    let harness = harness().await;
    for invalid in [" ", "\u{0007}", &"가".repeat(81)] {
        assert_eq!(
            create(&harness.app, invalid).await.0,
            StatusCode::UNPROCESSABLE_ENTITY
        );
    }
    let (_, project) = create(&harness.app, "Valid").await;
    let id = project["id"].as_i64().expect("project id");
    assert_eq!(
        call(
            &harness.app,
            Method::PATCH,
            "/v1/projects/999999",
            Some(json!({"name": "Missing"})),
            true,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        merge(&harness.app, id, id).await.0,
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(
        merge(&harness.app, id, 999999).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        merge(&harness.app, 999999, id).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &harness.app,
            Method::DELETE,
            &format!("/v1/projects/{id}"),
            None,
            true,
        )
        .await
        .0,
        StatusCode::METHOD_NOT_ALLOWED
    );
}

#[tokio::test]
async fn projects_merge_rewrites_references_and_preserves_activity_canvas_rows() {
    let harness = harness().await;
    ingest(
        &harness.app,
        "source",
        "one",
        "source-root",
        "source prompt",
    )
    .await;
    ingest(
        &harness.app,
        "target",
        "two",
        "target-root",
        "target prompt",
    )
    .await;
    let (_, projects) = call(&harness.app, Method::GET, "/v1/projects", None, true).await;
    let source = project_id(&projects, "source-root");
    let target = project_id(&projects, "target-root");
    harness
        .store
        .remember_conversation_route("codex", "remembered", source)
        .await
        .expect("source route");
    let nodes = harness.store.canvas_nodes().await.expect("nodes");
    harness
        .store
        .update_canvas_position(nodes[0].id, 137.0, 251.0)
        .await
        .expect("position");
    harness
        .store
        .create_canvas_edge(nodes[0].id, nodes[1].id)
        .await
        .expect("edge");
    let activities_before = immutable_activity_fields(
        serde_json::to_value(harness.store.activities().await.expect("activities"))
            .expect("activity JSON"),
    );
    let nodes_before = serde_json::to_value(harness.store.canvas_nodes().await.expect("nodes"))
        .expect("node JSON");
    let edges_before = serde_json::to_value(harness.store.canvas_edges().await.expect("edges"))
        .expect("edge JSON");

    assert_eq!(merge(&harness.app, source, target).await.0, StatusCode::OK);

    assert_eq!(
        immutable_activity_fields(
            serde_json::to_value(harness.store.activities().await.expect("activities"))
                .expect("activity JSON"),
        ),
        activities_before
    );
    assert_eq!(
        serde_json::to_value(harness.store.canvas_nodes().await.expect("nodes"))
            .expect("node JSON"),
        nodes_before
    );
    assert_eq!(
        serde_json::to_value(harness.store.canvas_edges().await.expect("edges"))
            .expect("edge JSON"),
        edges_before
    );
    assert!(
        !harness
            .store
            .remember_conversation_route("codex", "remembered", target)
            .await
            .expect("same target proves route rewrite")
    );
    let (_, listed) = call(&harness.app, Method::GET, "/v1/projects", None, true).await;
    assert_eq!(listed.as_array().expect("projects").len(), 1);
    assert_eq!(listed[0]["id"], target);
    assert_eq!(listed[0]["name"], "target-root");
    assert_eq!(listed[0]["origin_count"], 2);
    assert_eq!(listed[0]["activity_count"], 2);
    assert!(
        listed[0]["latest_activity_at_us"]
            .as_i64()
            .is_some_and(|time| time > 0)
    );
}

fn immutable_activity_fields(mut activities: serde_json::Value) -> serde_json::Value {
    for activity in activities.as_array_mut().expect("activity array") {
        activity
            .as_object_mut()
            .expect("activity object")
            .remove("project");
    }
    activities
}

#[tokio::test]
async fn projects_routes_all_require_bearer_authentication() {
    let harness = harness().await;
    for (method, uri, body) in [
        (Method::GET, "/v1/projects", None),
        (Method::POST, "/v1/projects", Some(json!({"name": "x"}))),
        (Method::PATCH, "/v1/projects/1", Some(json!({"name": "x"}))),
        (
            Method::POST,
            "/v1/projects/1/merge",
            Some(json!({"target_project_id": 2})),
        ),
    ] {
        assert_eq!(
            call(&harness.app, method, uri, body, false).await.0,
            StatusCode::UNAUTHORIZED
        );
    }
}
