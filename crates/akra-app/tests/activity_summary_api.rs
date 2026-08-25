use std::{
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use akra_core::ingress::{ActivityKind, IngressEvent, ResultEvent};
use akra_store::{
    ActivityAssignmentCommand, AssignmentDestination, FutureRouteAction, OriginRoutingCommand,
    RecordActivity, RecordResult, ResultSummaryFailureDisposition,
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
        assert_eq!(object.len(), 10);
        assert_eq!(object["activity_kind"], "user");
        assert_eq!(object["result_summary_status"], "unavailable");
        assert_eq!(object["prompt_summary"]["status"], "ready");
        assert_eq!(object["prompt_summary"]["mode"], "passthrough");
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

#[tokio::test]
async fn activity_kind_policy_applies_to_pages_details_counts_and_projects() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\kind-filter");
    let user = record_kind(
        &harness.store,
        cwd,
        "mixed-session",
        "user-turn",
        ActivityKind::User,
        100,
    )
    .await;
    record_kind(
        &harness.store,
        cwd,
        "mixed-session",
        "internal-turn",
        ActivityKind::Internal,
        200,
    )
    .await;

    let visibility = "include_subagent=true&include_internal=false";
    let (_, activities) = get(
        &harness.app,
        &format!("/v1/activities?scope=all&{visibility}"),
    )
    .await;
    let activities = activities.as_array().expect("activities");
    assert_eq!(ids(activities), vec![user]);
    assert_position(activities, user, 1, 1);

    let (_, count) = get(
        &harness.app,
        &format!("/v1/activities/count?scope=all&{visibility}"),
    )
    .await;
    assert_eq!(count["count"], 1);

    let (_, detail) = get(&harness.app, &format!("/v1/activities/{user}?{visibility}")).await;
    assert_eq!(detail["conversation_total"], 1);
    assert_eq!(
        detail["conversation"]
            .as_array()
            .expect("conversation")
            .len(),
        1
    );

    let (_, projects) = get(&harness.app, &format!("/v1/projects?{visibility}")).await;
    let projects = projects.as_array().expect("projects");
    assert_eq!(projects.len(), 1);
    assert_eq!(projects[0]["activity_count"], 1);
}

#[tokio::test]
async fn period_filters_keep_nodes_and_every_navigation_count_in_sync() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\period-filter");
    let now_us: i64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_micros()
        .try_into()
        .expect("timestamp fits i64");
    let recent = record(&harness.store, cwd, "period", "recent", Some(now_us)).await;
    let within_day_before_today = record(
        &harness.store,
        cwd,
        "period",
        "within-day-before-today",
        Some(now_us - 3 * 60 * 60 * 1_000_000),
    )
    .await;
    record(
        &harness.store,
        cwd,
        "period",
        "older-than-quarter",
        Some(now_us - 91 * 24 * 60 * 60 * 1_000_000),
    )
    .await;

    let (activity_status, activities) = get(
        &harness.app,
        "/v1/activities?scope=all&period=week&order=newest",
    )
    .await;
    assert_eq!(activity_status, StatusCode::OK);
    assert_eq!(
        ids(activities.as_array().expect("activities")),
        vec![within_day_before_today, recent]
    );

    let (_, count) = get(&harness.app, "/v1/activities/count?scope=all&period=week").await;
    assert_eq!(count["count"], 2);

    let (_, projects) = get(&harness.app, "/v1/projects?period=week").await;
    assert_eq!(projects[0]["activity_count"], 2);

    let (_, origins) = get(&harness.app, "/v1/origins?period=week").await;
    assert_eq!(origins[0]["activity_count"], 2);

    let today_start_us = now_us - 2 * 60 * 60 * 1_000_000;
    let (_, today) = get(
        &harness.app,
        &format!("/v1/activities?scope=all&period=today&start_at_us={today_start_us}&order=newest"),
    )
    .await;
    assert_eq!(ids(today.as_array().expect("today")), vec![recent]);
    let (_, today_count) = get(
        &harness.app,
        &format!("/v1/activities/count?scope=all&period=today&start_at_us={today_start_us}"),
    )
    .await;
    assert_eq!(today_count["count"], 1);
    let (_, today_projects) = get(
        &harness.app,
        &format!("/v1/projects?period=today&start_at_us={today_start_us}"),
    )
    .await;
    assert_eq!(today_projects[0]["activity_count"], 1);
    let (_, today_origins) = get(
        &harness.app,
        &format!("/v1/origins?period=today&start_at_us={today_start_us}"),
    )
    .await;
    assert_eq!(today_origins[0]["activity_count"], 1);

    let (_, rolling_day) = get(
        &harness.app,
        "/v1/activities?scope=all&period=day&order=newest",
    )
    .await;
    assert_eq!(
        ids(rolling_day.as_array().expect("rolling day")),
        vec![within_day_before_today, recent]
    );

    let (missing_today_start, invalid_today) =
        get(&harness.app, "/v1/activities?scope=all&period=today").await;
    assert_eq!(missing_today_start, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid_today["code"], "invalid_period");
    let (unexpected_start, invalid_day) = get(
        &harness.app,
        &format!("/v1/activities?scope=all&period=day&start_at_us={today_start_us}"),
    )
    .await;
    assert_eq!(unexpected_start, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid_day["code"], "invalid_period");

    let (invalid_status, invalid) =
        get(&harness.app, "/v1/activities?scope=all&period=calendar").await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["code"], "invalid_period");
}

#[tokio::test]
async fn failed_result_summary_can_only_be_regenerated_while_its_source_is_retained() {
    let harness = harness().await;
    let cwd = Path::new(r"C:\summary-regeneration");
    let captured_at_us: i64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time")
        .as_micros()
        .try_into()
        .expect("timestamp fits i64");
    let activity_id = record(
        &harness.store,
        cwd,
        "regeneration-session",
        "regeneration-turn",
        Some(captured_at_us),
    )
    .await;
    harness
        .store
        .capture_result(RecordResult::captured(
            ResultEvent::try_new(
                "codex",
                "regeneration-session",
                "regeneration-turn",
                cwd.to_string_lossy(),
                Some("The implementation failed its final verification.".to_owned()),
                Some("gpt-5.3-codex".to_owned()),
            )
            .expect("result event"),
            captured_at_us + 1,
        ))
        .await
        .expect("capture result");
    let claim = harness
        .store
        .claim_result_summary(captured_at_us + 2, 1_000_000)
        .await
        .expect("claim")
        .expect("pending result");
    assert_eq!(
        harness
            .store
            .fail_result_summary(
                &claim,
                "invalid output",
                akra_store::ResultSummaryErrorCode::InvalidOutput,
                None,
                captured_at_us + 3,
            )
            .await
            .expect("terminal failure"),
        ResultSummaryFailureDisposition::Failed
    );

    let (_, failed_detail) = get(&harness.app, &format!("/v1/activities/{activity_id}")).await;
    assert_eq!(failed_detail["result_summary"]["status"], "failed");
    assert_eq!(failed_detail["result_summary"]["can_regenerate"], true);

    let (scheduled, body) = call(
        &harness.app,
        Method::POST,
        &format!("/v1/activities/{activity_id}/result-summary/regenerate"),
        None,
        true,
    )
    .await;
    assert_eq!(scheduled, StatusCode::ACCEPTED);
    assert!(body.is_null());
    let (_, pending_detail) = get(&harness.app, &format!("/v1/activities/{activity_id}")).await;
    assert_eq!(pending_detail["result_summary"]["status"], "pending");
    assert_eq!(pending_detail["result_summary"]["can_regenerate"], false);

    let without_source = record(
        &harness.store,
        cwd,
        "regeneration-session",
        "no-source-turn",
        Some(captured_at_us),
    )
    .await;
    let (unavailable, error) = call(
        &harness.app,
        Method::POST,
        &format!("/v1/activities/{without_source}/result-summary/regenerate"),
        None,
        true,
    )
    .await;
    assert_eq!(unavailable, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(error["code"], "result_summary_regeneration_unavailable");
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

async fn record_kind(
    store: &akra_store::ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    kind: ActivityKind,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), turn, None)
        .expect("event")
        .with_activity_context(kind, None, None)
        .expect("activity context");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
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
