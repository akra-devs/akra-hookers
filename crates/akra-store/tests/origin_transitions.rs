use akra_store::{OriginRoutingCommand, ProjectDestination, StoreError};
#[path = "support/origin_transition.rs"]
mod support;
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn dedicated_reassignment_moves_history_and_future_without_overrides() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "dedicated");
    let first = record(&store, &cwd, "session", "one", 20).await;
    let second = record(&store, &cwd, "session", "two", 10).await;
    let (origin_id, source_project) = origin_and_project(&pool, first).await;
    let target = store.create_project("Target").await.expect("target").id;
    let nodes = store.canvas_nodes().await.expect("nodes");
    store
        .update_canvas_position(nodes[0].id, 33.0, 44.0)
        .await
        .expect("position");
    store
        .create_canvas_edge(nodes[0].id, nodes[1].id)
        .await
        .expect("edge");
    let immutable_before = immutable_snapshot(&pool).await;

    let moved = store
        .configure_origin(
            origin_id,
            OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(target), true),
        )
        .await
        .expect("move origin");

    assert_eq!(moved.default_project_id, Some(target));
    assert_eq!(moved.setup_state, "confirmed");
    assert_eq!(effective_project(&pool, first).await, Some(target));
    assert_eq!(effective_project(&pool, second).await, Some(target));
    assert_eq!(assignment_count(&pool, origin_id).await, 0);
    assert_eq!(immutable_snapshot(&pool).await, immutable_before);
    let origin_before: String = sqlx::query_scalar(
        "SELECT json_array(routing_mode, default_project_id, setup_state, updated_at_us)
         FROM activity_origins WHERE id = ?",
    )
    .bind(origin_id)
    .fetch_one(&pool)
    .await
    .expect("origin state");
    assert_eq!(
        store
            .configure_origin(
                origin_id,
                OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(target), true,),
            )
            .await
            .expect("same-target no-op")
            .default_project_id,
        Some(target)
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT json_array(routing_mode, default_project_id, setup_state, updated_at_us)
             FROM activity_origins WHERE id = ?",
        )
        .bind(origin_id)
        .fetch_one(&pool)
        .await
        .expect("same-target state"),
        origin_before
    );
    let future = record(&store, &cwd, "session", "three", 5).await;
    assert_eq!(effective_project(&pool, future).await, Some(target));
    assert_ne!(source_project, target);
}

#[tokio::test]
async fn dedicated_shared_dedicated_round_trip_materializes_then_removes_overrides() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "round-trip");
    let first = record(&store, &cwd, "remembered", "one", 1).await;
    let second = record(&store, &cwd, "other", "two", 2).await;
    let (origin_id, source_project) = origin_and_project(&pool, first).await;

    let shared = store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("convert shared");

    assert_eq!(shared.routing_mode, "shared");
    assert_eq!(shared.default_project_id, None);
    assert_eq!(effective_project(&pool, first).await, Some(source_project));
    assert_eq!(effective_project(&pool, second).await, Some(source_project));
    assert_eq!(assignment_count(&pool, origin_id).await, 2);
    let inbox = record(&store, &cwd, "unrouted", "three", 3).await;
    assert_eq!(effective_project(&pool, inbox).await, None);
    let target = store.create_project("Target").await.expect("target").id;
    sqlx::query(
        "UPDATE activity_project_assignments SET project_id = ?
         WHERE activity_event_id = ?",
    )
    .bind(target)
    .bind(first)
    .execute(&pool)
    .await
    .expect("shared override");
    store
        .remember_conversation_route("codex", "remembered", source_project)
        .await
        .expect("future route");
    let immutable_before = immutable_snapshot(&pool).await;

    let dedicated = store
        .configure_origin(
            origin_id,
            OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(target), true),
        )
        .await
        .expect("convert dedicated");

    assert_eq!(dedicated.routing_mode, "dedicated");
    assert_eq!(dedicated.default_project_id, Some(target));
    assert_eq!(assignment_count(&pool, origin_id).await, 0);
    for activity_id in [first, second, inbox] {
        assert_eq!(effective_project(&pool, activity_id).await, Some(target));
    }
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM conversation_routes
             WHERE provider = 'codex' AND provider_session_id = 'remembered'",
        )
        .fetch_one(&pool)
        .await
        .expect("routes"),
        0
    );
    assert_eq!(immutable_snapshot(&pool).await, immutable_before);
    let future = record(&store, &cwd, "future", "four", 4).await;
    assert_eq!(effective_project(&pool, future).await, Some(target));
}

#[tokio::test]
async fn rename_confirm_and_invalid_transitions_are_atomic() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "suggested");
    let activity = record(&store, &cwd, "session", "turn", 1).await;
    let (origin_id, suggested_project) = origin_and_project(&pool, activity).await;

    let renamed = store
        .configure_origin(
            origin_id,
            OriginRoutingCommand::dedicated(
                ProjectDestination::NewProjectName("이름 변경".into()),
                true,
            ),
        )
        .await
        .expect("rename suggestion");
    assert_eq!(renamed.default_project_id, Some(suggested_project));
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM projects WHERE id = ?")
            .bind(suggested_project)
            .fetch_one(&pool)
            .await
            .expect("renamed project"),
        "이름 변경"
    );
    let before: (String, Option<i64>, String) = sqlx::query_as(
        "SELECT routing_mode, default_project_id, setup_state
         FROM activity_origins WHERE id = ?",
    )
    .bind(origin_id)
    .fetch_one(&pool)
    .await
    .expect("origin state");
    assert!(matches!(
        store
            .configure_origin(origin_id, OriginRoutingCommand::shared(false))
            .await,
        Err(StoreError::InvalidOriginTransition(_))
    ));
    assert!(matches!(
        store
            .configure_origin(
                origin_id,
                OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(999_999), true,),
            )
            .await,
        Err(StoreError::ProjectNotFound(999_999))
    ));
    assert!(matches!(
        store
            .configure_origin(999_999, OriginRoutingCommand::shared(true))
            .await,
        Err(StoreError::OriginNotFound(999_999))
    ));
    assert_eq!(
        sqlx::query_as::<_, (String, Option<i64>, String)>(
            "SELECT routing_mode, default_project_id, setup_state
             FROM activity_origins WHERE id = ?",
        )
        .bind(origin_id)
        .fetch_one(&pool)
        .await
        .expect("unchanged origin"),
        before
    );
}

#[tokio::test]
async fn suggested_project_rename_failure_preserves_origin_and_project_state() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "rename-failure");
    let activity = record(&store, &cwd, "session", "turn", 1).await;
    let (origin_id, suggested_project) = origin_and_project(&pool, activity).await;
    let origin_before: (String, Option<i64>, String, i64) = sqlx::query_as(
        "SELECT routing_mode, default_project_id, setup_state, updated_at_us
         FROM activity_origins WHERE id = ?",
    )
    .bind(origin_id)
    .fetch_one(&pool)
    .await
    .expect("origin state");
    let project_before: String = sqlx::query_scalar("SELECT name FROM projects WHERE id = ?")
        .bind(suggested_project)
        .fetch_one(&pool)
        .await
        .expect("project name");
    sqlx::query(
        r#"
        CREATE TRIGGER reject_suggested_project_rename
        BEFORE UPDATE OF name ON projects
        WHEN NEW.name = 'blocked'
        BEGIN
            SELECT RAISE(ABORT, 'injected rename failure');
        END
        "#,
    )
    .execute(&pool)
    .await
    .expect("failure trigger");

    assert!(matches!(
        store
            .configure_origin(
                origin_id,
                OriginRoutingCommand::dedicated(
                    ProjectDestination::NewProjectName("blocked".into()),
                    true,
                ),
            )
            .await,
        Err(StoreError::Sqlite(_))
    ));
    assert_eq!(
        sqlx::query_as::<_, (String, Option<i64>, String, i64)>(
            "SELECT routing_mode, default_project_id, setup_state, updated_at_us
             FROM activity_origins WHERE id = ?",
        )
        .bind(origin_id)
        .fetch_one(&pool)
        .await
        .expect("unchanged origin"),
        origin_before
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT name FROM projects WHERE id = ?")
            .bind(suggested_project)
            .fetch_one(&pool)
            .await
            .expect("unchanged project"),
        project_before
    );
    assert_eq!(assignment_count(&pool, origin_id).await, 0);
}
