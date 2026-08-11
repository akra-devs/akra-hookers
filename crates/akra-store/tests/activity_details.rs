use std::path::Path;

use akra_core::ingress::IngressEvent;
use akra_store::{
    ActivityAssignmentCommand, ActivityScope, ActivityTimeProvenance, AssignmentDestination,
    FutureRouteAction, OriginRoutingCommand, RecordActivity, StoreError,
};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

#[path = "support/origin_transition.rs"]
mod support;
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn detail_keeps_full_metadata_and_complete_cross_project_timeline() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "detail");
    let first = record(&store, &cwd, "conversation", "one", 100).await;
    let full_prompt = "전체 프롬프트 ".repeat(300);
    let selected = record_full(&store, &cwd, "two", &full_prompt, 200).await;
    let third = record(&store, &cwd, "conversation", "three", 300).await;
    let origin_id = origin_and_project(&pool, first).await.0;
    store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared");
    let project_a = store.create_project("A").await.expect("A").id;
    let project_b = store.create_project("B").await.expect("B").id;
    for (activity_id, destination) in [
        (first, AssignmentDestination::ProjectId(project_a)),
        (selected, AssignmentDestination::ProjectId(project_b)),
        (third, AssignmentDestination::Inbox),
    ] {
        store
            .assign_activities(ActivityAssignmentCommand::new(
                vec![activity_id],
                destination,
                FutureRouteAction::Unchanged,
            ))
            .await
            .expect("assignment");
    }
    let selected_node: i64 =
        sqlx::query_scalar("SELECT id FROM canvas_nodes WHERE activity_event_id = ?")
            .bind(selected)
            .fetch_one(&pool)
            .await
            .expect("selected node");
    store
        .delete_canvas_node(selected_node)
        .await
        .expect("delete canvas node");

    let detail = store.activity_detail(selected).await.expect("detail");
    assert_eq!(detail.id, selected);
    assert_eq!(detail.provider, "codex");
    assert_eq!(detail.prompt, full_prompt);
    assert_eq!(detail.project.as_ref().expect("project").name, "B");
    assert_eq!(
        detail.submitted_cwd.as_deref(),
        Some(cwd.to_string_lossy().as_ref())
    );
    assert_eq!(detail.origin.id, origin_id);
    assert_eq!(detail.origin.kind, "directory");
    assert_eq!(detail.origin.display_path, cwd.to_string_lossy());
    assert_eq!(detail.origin.resolution_source, "captured");
    assert_eq!(detail.origin.activity_count, 3);
    assert_eq!(detail.technical.session_id, "conversation");
    assert_eq!(detail.technical.turn_id, "two");
    assert!(detail.captured_at.value.is_some());
    assert!(detail.first_recorded_at.value.is_some());
    assert!(!detail.on_canvas);
    assert_eq!(
        detail
            .conversation
            .iter()
            .map(|turn| turn.id)
            .collect::<Vec<_>>(),
        vec![first, selected, third]
    );
    assert_eq!(
        detail
            .conversation
            .iter()
            .map(|turn| turn.project.as_ref().map(|project| project.name.as_str()))
            .collect::<Vec<_>>(),
        vec![Some("A"), Some("B"), None]
    );
    assert_eq!(
        detail
            .conversation
            .iter()
            .map(|turn| (turn.on_canvas, turn.selected))
            .collect::<Vec<_>>(),
        vec![(true, false), (false, true), (true, false)]
    );
    assert_eq!(effective_project(&pool, first).await, Some(project_a));
    assert_eq!(assignment_count(&pool, origin_id).await, 2);
    assert!(!immutable_snapshot(&pool).await.is_empty());
    let summary = serde_json::to_value(
        store
            .activity_summaries(ActivityScope::All)
            .await
            .expect("summaries"),
    )
    .expect("summary JSON");
    assert!(summary[0].get("technical").is_none());
    assert!(summary[0].get("session_id").is_none());
    assert!(summary[0].get("turn_id").is_none());
}

#[tokio::test]
async fn migrated_legacy_detail_keeps_unavailable_capture_separate_from_detection() {
    let directory = TempDir::new().expect("directory");
    let database_path = directory.path().join("legacy-detail.sqlite");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&database_path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .expect("legacy pool");
    sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
        .execute(&pool)
        .await
        .expect("v1 schema");
    sqlx::raw_sql(
        "INSERT INTO projects (id, identity, display_path)
         VALUES (7, 'legacy-origin', 'C:\\detected\\legacy');
         INSERT INTO activity_events (
             id, provider, provider_session_id, provider_turn_id,
             project_identity, prompt, created_at
         ) VALUES (
             41, 'codex', 'legacy-session', 'legacy-turn',
             'legacy-origin', 'legacy full prompt', '2025-01-02 03:04:05'
         );",
    )
    .execute(&pool)
    .await
    .expect("legacy fixture");
    drop(pool);
    let store = akra_store::ActivityStore::open(&database_path)
        .await
        .expect("store");
    store.migrate().await.expect("migration");

    let detail = store.activity_detail(41).await.expect("legacy detail");
    assert!(detail.submitted_cwd.is_none());
    assert!(detail.captured_at.value.is_none());
    assert_eq!(
        detail.captured_at.provenance,
        ActivityTimeProvenance::Unknown
    );
    assert_eq!(
        detail.first_recorded_at.value.as_deref(),
        Some("2025-01-02T03:04:05Z")
    );
    assert_eq!(
        detail.first_recorded_at.provenance,
        ActivityTimeProvenance::LegacyRecorded
    );
    assert_eq!(detail.origin.display_path, r"C:\detected\legacy");
    assert_eq!(detail.origin.resolution_source, "legacy_migrated");
    assert_eq!(detail.origin.activity_count, 1);
    assert_eq!(detail.technical.session_id, "legacy-session");
    assert_eq!(detail.technical.turn_id, "legacy-turn");
    assert_eq!(detail.conversation.len(), 1);
    assert!(matches!(
        store.activity_detail(999_999).await,
        Err(StoreError::ActivityNotFound(999_999))
    ));
}

async fn record_full(
    store: &akra_store::ActivityStore,
    cwd: &Path,
    turn: &str,
    prompt: &str,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new(
        "codex",
        "conversation",
        turn,
        cwd.to_string_lossy(),
        prompt,
        None,
    )
    .expect("event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}
