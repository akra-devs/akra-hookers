use std::path::Path;

use akra_store::{
    ActivityAssignmentCommand, AssignmentDestination, FutureRouteAction, OriginRoutingCommand,
};
use axum::http::{Method, StatusCode};
use serde_json::Value;

use crate::{
    api_harness::{call, create_project, harness},
    detail_support::{legacy_app, record_captured},
};

#[tokio::test]
async fn detail_returns_full_selected_metadata_and_complete_mixed_timeline() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\activity-detail");
    let first = record_captured(&harness.store, cwd, "same", "one", "first", 100).await;
    let full_prompt = "상세 패널 전체 프롬프트 ".repeat(300);
    let selected = record_captured(&harness.store, cwd, "same", "two", &full_prompt, 200).await;
    let third = record_captured(&harness.store, cwd, "same", "three", "third", 300).await;
    let origin_id = harness.store.origins().await.expect("origins")[0].id;
    harness
        .store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared");
    let (_, project_a) = create_project(&harness.app, "A").await;
    let (_, project_b) = create_project(&harness.app, "B").await;
    let project_a = project_a["id"].as_i64().expect("A");
    let project_b = project_b["id"].as_i64().expect("B");
    for (activity_id, destination) in [
        (first, AssignmentDestination::ProjectId(project_a)),
        (selected, AssignmentDestination::ProjectId(project_b)),
        (third, AssignmentDestination::Inbox),
    ] {
        harness
            .store
            .assign_activities(ActivityAssignmentCommand::new(
                vec![activity_id],
                destination,
                FutureRouteAction::Unchanged,
            ))
            .await
            .expect("assignment");
    }
    assert_eq!(
        harness
            .store
            .projects()
            .await
            .expect("projects")
            .into_iter()
            .find(|project| project.id == project_a)
            .expect("A summary")
            .activity_count,
        1
    );
    let node = harness
        .store
        .canvas_nodes()
        .await
        .expect("nodes")
        .into_iter()
        .find(|node| node.activity_event_id == selected)
        .expect("selected node");
    harness
        .store
        .delete_canvas_node(node.id)
        .await
        .expect("delete selected node");

    let (status, detail) = get(&harness.app, &format!("/v1/activities/{selected}"), true).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(detail["id"], selected);
    assert_eq!(detail["provider"], "codex");
    assert_eq!(detail["prompt"], full_prompt);
    assert_eq!(detail["project"]["name"], "B");
    assert_eq!(detail["submitted_cwd"], cwd.to_string_lossy().as_ref());
    assert_eq!(
        detail["origin"]["display_path"],
        cwd.to_string_lossy().as_ref()
    );
    assert_eq!(detail["origin"]["kind"], "unresolved");
    assert_eq!(detail["origin"]["resolution_source"], "captured");
    assert_eq!(detail["origin"]["activity_count"], 3);
    assert_eq!(detail["technical"]["session_id"], "same");
    assert_eq!(detail["technical"]["turn_id"], "two");
    assert_eq!(detail["selected_turn"]["id"], selected);
    assert_eq!(detail["selected_turn"]["selected"], true);
    assert!(detail["captured_at"]["value"].is_string());
    assert!(detail["first_recorded_at"]["value"].is_string());
    assert_eq!(detail["on_canvas"], false);
    let conversation = detail["conversation"].as_array().expect("conversation");
    assert_eq!(ids(conversation), vec![first, selected, third]);
    assert_eq!(detail["conversation_total"], 3);
    assert_eq!(detail["conversation_index"], 2);
    assert_eq!(detail["conversation_has_more"], false);
    assert_eq!(
        conversation
            .iter()
            .map(|turn| turn["project"]["name"].as_str())
            .collect::<Vec<_>>(),
        vec![Some("A"), Some("B"), None]
    );
    assert_eq!(
        conversation
            .iter()
            .map(|turn| (turn["on_canvas"].as_bool(), turn["selected"].as_bool()))
            .collect::<Vec<_>>(),
        vec![
            (Some(true), Some(false)),
            (Some(false), Some(true)),
            (Some(true), Some(false))
        ]
    );
    let (_, first_page) = get(
        &harness.app,
        &format!("/v1/activities/{selected}?conversation_limit=2"),
        true,
    )
    .await;
    assert_eq!(
        ids(first_page["conversation"].as_array().expect("page")),
        vec![first, selected]
    );
    assert_eq!(first_page["conversation_total"], 3);
    assert_eq!(first_page["conversation_has_more"], true);
    let (_, second_page) = get(
        &harness.app,
        &format!("/v1/activities/{selected}?conversation_limit=2&conversation_after_id={selected}"),
        true,
    )
    .await;
    assert_eq!(
        ids(second_page["conversation"].as_array().expect("page")),
        vec![third]
    );
    assert_eq!(second_page["conversation_has_more"], false);
    let (_, offset_page) = get(
        &harness.app,
        &format!("/v1/activities/{selected}?conversation_limit=1&conversation_offset=1"),
        true,
    )
    .await;
    assert_eq!(
        ids(offset_page["conversation"].as_array().expect("offset page")),
        vec![selected]
    );
    assert_eq!(offset_page["conversation_has_more"], true);
    let (_, late_selection) = get(
        &harness.app,
        &format!("/v1/activities/{third}?conversation_limit=2"),
        true,
    )
    .await;
    assert_eq!(
        ids(late_selection["conversation"].as_array().expect("page")),
        vec![first, selected]
    );
    assert_eq!(late_selection["selected_turn"]["id"], third);
    assert_eq!(late_selection["conversation_has_more"], true);
    let (_, summaries) = get(&harness.app, "/v1/activities?scope=all", true).await;
    let selected_summary = summaries
        .as_array()
        .expect("summaries")
        .iter()
        .find(|summary| summary["id"] == selected)
        .expect("selected summary");
    assert_ne!(selected_summary["prompt"], full_prompt);
    assert!(
        selected_summary["prompt"]
            .as_str()
            .expect("prompt preview")
            .ends_with('…')
    );
    for summary in summaries.as_array().expect("summaries") {
        for forbidden in [
            "technical",
            "session_id",
            "turn_id",
            "submitted_cwd",
            "origin",
        ] {
            assert!(summary.get(forbidden).is_none(), "{forbidden}");
        }
    }
    assert_eq!(
        get(&harness.app, "/v1/activities/999999", true).await.0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        get(&harness.app, &format!("/v1/activities/{selected}"), false)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn delete_tombstones_the_activity_and_removes_it_from_the_http_contract() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\activity-delete");
    let first = record_captured(&harness.store, cwd, "same", "one", "first", 100).await;
    let deleted = record_captured(&harness.store, cwd, "same", "two", "deleted", 200).await;

    let (status, body) = call(
        &harness.app,
        Method::DELETE,
        &format!("/v1/activities/{deleted}"),
        None,
        true,
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    assert!(body.is_null());
    assert_eq!(
        get(&harness.app, &format!("/v1/activities/{deleted}"), true)
            .await
            .0,
        StatusCode::NOT_FOUND
    );
    let (_, activities) = get(&harness.app, "/v1/activities?scope=all", true).await;
    assert_eq!(ids(activities.as_array().expect("activities")), vec![first]);
    let (_, count) = get(&harness.app, "/v1/activities/count?scope=all", true).await;
    assert_eq!(count["count"], 1);
    assert_eq!(
        call(
            &harness.app,
            Method::DELETE,
            &format!("/v1/activities/{deleted}"),
            None,
            true,
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        call(
            &harness.app,
            Method::DELETE,
            &format!("/v1/activities/{first}"),
            None,
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn migrated_detail_keeps_capture_and_submitted_cwd_unavailable() {
    let (_directory, app) = legacy_app().await;
    let (status, detail) = get(&app, "/v1/activities/41", true).await;
    assert_eq!(status, StatusCode::OK);
    assert!(detail["submitted_cwd"].is_null());
    assert!(detail["captured_at"]["value"].is_null());
    assert_eq!(detail["captured_at"]["provenance"], "unknown");
    assert_eq!(detail["first_recorded_at"]["value"], "2025-01-02T03:04:05Z");
    assert_eq!(detail["first_recorded_at"]["provenance"], "legacy_recorded");
    assert_eq!(detail["origin"]["display_path"], r"C:\detected\legacy");
    assert_eq!(detail["origin"]["resolution_source"], "legacy_migrated");
    assert_eq!(detail["technical"]["session_id"], "legacy-session");
    assert_eq!(detail["technical"]["turn_id"], "legacy-turn");
}

async fn get(app: &axum::Router, uri: &str, authorized: bool) -> (StatusCode, Value) {
    call(app, Method::GET, uri, None, authorized).await
}

fn ids(turns: &[Value]) -> Vec<i64> {
    turns
        .iter()
        .map(|turn| turn["id"].as_i64().expect("turn id"))
        .collect()
}
