use akra_git::ProjectIdentity;
use akra_store::RecordActivity;

#[path = "activity_routing/support.rs"]
mod support;

use support::{assignment, event, harness, insert_project, make_shared, record, working_directory};

#[tokio::test]
async fn unseen_origin_is_unconfirmed_dedicated_with_final_component_project() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "workspace");

    let id = record(&store, &cwd, "session", "turn", "prompt", 50).await;

    let row: (String, String, String, i64, i64) = sqlx::query_as(
        "SELECT activity_origins.routing_mode, activity_origins.setup_state, projects.name,
                (SELECT COUNT(*) FROM activity_project_assignments WHERE activity_event_id = ?),
                (SELECT COUNT(*) FROM canvas_nodes WHERE activity_event_id = ?)
         FROM activity_origins
         JOIN projects ON projects.id = activity_origins.default_project_id",
    )
    .bind(id)
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("origin state");
    assert_eq!(
        row,
        (
            "dedicated".into(),
            "unconfirmed".into(),
            "workspace".into(),
            0,
            1
        )
    );
}

#[tokio::test]
async fn dedicated_default_wins_over_a_conversation_route() {
    let (directory, store, pool) = harness().await;
    let routed_project = insert_project(&pool, "Routed").await;
    assert!(
        store
            .remember_conversation_route("codex", "session", routed_project)
            .await
            .expect("route")
    );
    let cwd = working_directory(&directory, "dedicated");

    let id = record(&store, &cwd, "session", "turn", "prompt", 10).await;

    let default_project: i64 = sqlx::query_scalar(
        "SELECT default_project_id FROM activity_origins
         JOIN activity_events ON activity_events.origin_id = activity_origins.id
         WHERE activity_events.id = ?",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .expect("dedicated default");
    assert_ne!(default_project, routed_project);
    assert_eq!(assignment(&pool, id).await, None);
}

#[tokio::test]
async fn shared_route_materializes_assignment_and_missing_route_stays_inbox() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "shared-root");
    record(&store, &cwd, "setup", "turn", "setup", 1).await;
    make_shared(&pool).await;
    let routed_project = insert_project(&pool, "Routed").await;
    store
        .remember_conversation_route("codex", "routed-session", routed_project)
        .await
        .expect("route");

    let routed = record(&store, &cwd, "routed-session", "turn", "routed", 2).await;
    let inbox = record(&store, &cwd, "inbox-session", "turn", "inbox", 3).await;

    assert_eq!(assignment(&pool, routed).await, Some(routed_project));
    assert_eq!(assignment(&pool, inbox).await, None);
}

#[tokio::test]
async fn dedupe_wins_before_reclassification_and_never_recreates_deleted_node() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "immutable");
    let first = record(&store, &cwd, "session", "turn", "original", 10).await;
    let original_assignment = assignment(&pool, first).await;
    sqlx::query("DELETE FROM canvas_nodes WHERE activity_event_id = ?")
        .bind(first)
        .execute(&pool)
        .await
        .expect("delete canvas node");
    make_shared(&pool).await;
    let target = insert_project(&pool, "Replacement").await;
    store
        .remember_conversation_route("codex", "session", target)
        .await
        .expect("route");

    let duplicate = record(&store, &cwd, "session", "turn", "changed", 999).await;

    let row: (String, Option<i64>, i64, i64) = sqlx::query_as(
        "SELECT prompt, captured_at_us, global_sequence,
                (SELECT COUNT(*) FROM canvas_nodes WHERE activity_event_id = activity_events.id)
         FROM activity_events WHERE id = ?",
    )
    .bind(first)
    .fetch_one(&pool)
    .await
    .expect("immutable event");
    assert_eq!(duplicate, first);
    assert_eq!(row, ("original".into(), Some(10), 1, 0));
    assert_eq!(assignment(&pool, first).await, original_assignment);
}

#[tokio::test]
async fn route_same_target_is_a_true_noop_and_commit_order_defines_future() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "shared");
    record(&store, &cwd, "setup", "turn", "setup", 1).await;
    make_shared(&pool).await;
    let target = insert_project(&pool, "Target").await;

    let before = record(&store, &cwd, "future", "before", "before", 200).await;
    assert!(
        store
            .remember_conversation_route("codex", "future", target)
            .await
            .expect("new route")
    );
    assert!(
        !store
            .remember_conversation_route("codex", "future", target)
            .await
            .expect("same route")
    );
    let after = record(&store, &cwd, "future", "after", "after", 100).await;

    assert_eq!(assignment(&pool, before).await, None);
    assert_eq!(assignment(&pool, after).await, Some(target));
    let sequences: Vec<(String, i64)> = sqlx::query_as(
        "SELECT provider_turn_id, global_sequence FROM activity_events
         WHERE provider_session_id = 'future' ORDER BY global_sequence",
    )
    .fetch_all(&pool)
    .await
    .expect("sequences");
    assert_eq!(sequences, vec![("before".into(), 2), ("after".into(), 3)]);
}

#[tokio::test]
async fn legacy_command_has_null_capture_time_and_legacy_resolved_origin() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "legacy");
    let event = event(&cwd, "legacy", "turn", "legacy");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
        .expect("origin")
        .origin;

    store
        .record(RecordActivity::legacy_resolved(event, origin))
        .await
        .expect("record");

    let row: (Option<i64>, Option<String>, String) = sqlx::query_as(
        "SELECT captured_at_us, captured_at_provenance, resolution_source
         FROM activity_events JOIN activity_origins ON activity_origins.id = origin_id",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy row");
    assert_eq!(row, (None, None, "legacy_resolved".into()));
}
