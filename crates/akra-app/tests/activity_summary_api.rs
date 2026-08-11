use std::path::Path;

use akra_core::ingress::IngressEvent;
use akra_store::{
    ActivityAssignmentCommand, AssignmentDestination, FutureRouteAction, OriginRoutingCommand,
    RecordActivity,
};
use axum::http::{Method, StatusCode};
use serde_json::Value;

#[path = "support/api_harness.rs"]
mod api_harness;
use api_harness::{call, create_project, harness};

#[tokio::test]
async fn activities_scopes_omit_detail_metadata_and_keep_global_numbering() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\summary");
    let first = record(&harness.store, cwd, "same", "captured-300", Some(300)).await;
    let second = record(&harness.store, cwd, "same", "captured-100", Some(100)).await;
    let legacy = record(&harness.store, cwd, "same", "legacy", None).await;
    let origin_id = harness.store.origins().await.expect("origins")[0].id;
    harness
        .store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared");
    let (_, project) = create_project(&harness.app, "Scoped").await;
    let project_id = project["id"].as_i64().expect("project");
    harness
        .store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![first, second],
            AssignmentDestination::ProjectId(project_id),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("project assignment");
    harness
        .store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![legacy],
            AssignmentDestination::Inbox,
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("Inbox");
    let second_node = harness
        .store
        .canvas_nodes()
        .await
        .expect("nodes")
        .into_iter()
        .find(|node| node.activity_event_id == second)
        .expect("second node")
        .id;
    harness
        .store
        .delete_canvas_node(second_node)
        .await
        .expect("delete node");

    let all = get(&harness.app, "/v1/activities?scope=all").await;
    let project = get(
        &harness.app,
        &format!("/v1/activities?scope=project&project_id={project_id}"),
    )
    .await;
    let inbox = get(&harness.app, "/v1/activities?scope=inbox").await;
    assert_eq!(all.0, StatusCode::OK);
    assert_eq!(project.0, StatusCode::OK);
    assert_eq!(inbox.0, StatusCode::OK);
    let all = all.1.as_array().expect("all");
    let project = project.1.as_array().expect("project");
    let inbox = inbox.1.as_array().expect("Inbox");
    assert_eq!(ids(all), vec![first, second, legacy]);
    assert_eq!(ids(project), vec![first, second]);
    assert_eq!(ids(inbox), vec![legacy]);
    assert_position(all, second, 1, 3);
    assert_position(all, first, 2, 3);
    assert_position(all, legacy, 3, 3);
    assert_position(project, first, 2, 3);
    assert_position(project, second, 1, 3);
    assert_position(inbox, legacy, 3, 3);
    assert_eq!(find(all, first)["time"]["provenance"], "captured");
    assert_eq!(
        find(all, first)["time"]["value"],
        "1970-01-01T00:00:00.0003Z"
    );
    assert_eq!(find(all, legacy)["time"]["provenance"], "legacy_recorded");
    assert!(find(all, legacy)["time"]["value"].is_string());
    assert_eq!(find(all, first)["project"]["name"], "Scoped");
    assert!(find(all, legacy)["project"].is_null());
    for activity in all {
        let object = activity.as_object().expect("summary");
        assert_eq!(object.len(), 7);
        for forbidden in ["session_id", "turn_id", "cwd", "origin", "global_sequence"] {
            assert!(
                object.get(forbidden).is_none(),
                "{forbidden} must be omitted"
            );
        }
    }
}

#[tokio::test]
async fn activities_use_bounded_cursor_pages_without_overlap() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\paged-summary");
    for index in 1..=5 {
        record(
            &harness.store,
            cwd,
            "paged",
            &format!("turn-{index}"),
            Some(index),
        )
        .await;
    }

    let (first_status, first_page) = get(&harness.app, "/v1/activities?scope=all&limit=2").await;
    assert_eq!(first_status, StatusCode::OK);
    let first_page = first_page.as_array().expect("first page");
    assert_eq!(first_page.len(), 2);
    let cursor = first_page[1]["id"].as_i64().expect("cursor");
    let (second_status, second_page) = get(
        &harness.app,
        &format!("/v1/activities?scope=all&limit=2&after_id={cursor}"),
    )
    .await;
    assert_eq!(second_status, StatusCode::OK);
    let second_page = second_page.as_array().expect("second page");
    assert_eq!(second_page.len(), 2);
    assert!(
        ids(first_page)
            .into_iter()
            .all(|id| !ids(second_page).contains(&id))
    );
    let (_, newest) = get(
        &harness.app,
        "/v1/activities?scope=all&limit=2&order=newest",
    )
    .await;
    assert_eq!(ids(newest.as_array().expect("newest page")), vec![5, 4]);
    let (_, older) = get(
        &harness.app,
        "/v1/activities?scope=all&limit=2&order=newest&after_id=4",
    )
    .await;
    assert_eq!(ids(older.as_array().expect("older page")), vec![3, 2]);
    let (_, count) = get(&harness.app, "/v1/activities/count?scope=all").await;
    assert_eq!(count["count"], 5);

    let (invalid_status, invalid) = get(&harness.app, "/v1/activities?scope=all&limit=201").await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["code"], "invalid_pagination");
}

#[tokio::test]
async fn activities_scope_validation_and_authentication_are_explicit() {
    let harness = harness().await;
    let (_, project) = create_project(&harness.app, "Exists").await;
    let project_id = project["id"].as_i64().expect("project");
    for (uri, expected) in [
        ("/v1/activities", StatusCode::UNPROCESSABLE_ENTITY),
        (
            "/v1/activities?scope=invalid",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?scope=project",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?scope=project&project_id=abc",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?scope=all&project_id=1",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?scope=inbox&project_id=1",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?project=legacy",
            StatusCode::UNPROCESSABLE_ENTITY,
        ),
        (
            "/v1/activities?scope=project&project_id=999999",
            StatusCode::NOT_FOUND,
        ),
    ] {
        assert_eq!(get(&harness.app, uri).await.0, expected, "{uri}");
    }
    assert_eq!(
        call(
            &harness.app,
            Method::GET,
            &format!("/v1/activities?scope=project&project_id={project_id}"),
            None,
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}

async fn get(app: &axum::Router, uri: &str) -> (StatusCode, Value) {
    call(app, Method::GET, uri, None, true).await
}

async fn record(
    store: &akra_store::ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    captured_at_us: Option<i64>,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), turn, None)
        .expect("event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    let command = match captured_at_us {
        Some(captured_at_us) => RecordActivity::captured(event, origin, captured_at_us),
        None => RecordActivity::legacy_resolved(event, origin),
    };
    store.record(command).await.expect("record")
}

fn ids(activities: &[Value]) -> Vec<i64> {
    activities
        .iter()
        .map(|activity| activity["id"].as_i64().expect("id"))
        .collect()
}

fn find(activities: &[Value], activity_id: i64) -> &Value {
    activities
        .iter()
        .find(|activity| activity["id"] == activity_id)
        .expect("activity")
}

fn assert_position(activities: &[Value], activity_id: i64, index: i64, total: i64) {
    let activity = find(activities, activity_id);
    assert_eq!(activity["conversation_index"], index);
    assert_eq!(activity["conversation_total"], total);
}
