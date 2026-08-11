use akra_store::{
    ActivityAssignmentCommand, AssignmentDestination, FutureRouteAction, OriginRoutingCommand,
    StoreError,
};

#[path = "support/origin_transition.rs"]
mod support;
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn invalid_and_mixed_selections_roll_back_every_assignment_route_and_project() {
    let (directory, store, pool) = harness().await;
    let shared_cwd = working_directory(&directory, "shared");
    let dedicated_cwd = working_directory(&directory, "dedicated");
    let shared_a = record(&store, &shared_cwd, "a", "one", 1).await;
    let shared_b = record(&store, &shared_cwd, "b", "one", 2).await;
    let dedicated = record(&store, &dedicated_cwd, "a", "two", 3).await;
    let shared_origin = origin_and_project(&pool, shared_a).await.0;
    store
        .configure_origin(shared_origin, OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    let target = store.create_project("Target").await.expect("target").id;
    let before = state_snapshot(&pool).await;

    for command in [
        ActivityAssignmentCommand::new(
            vec![],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ),
        ActivityAssignmentCommand::new(
            vec![shared_a, dedicated],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ),
        ActivityAssignmentCommand::new(
            vec![shared_a, shared_b],
            AssignmentDestination::NewProjectName("Must roll back".into()),
            FutureRouteAction::Set,
        ),
        ActivityAssignmentCommand::new(
            vec![shared_a],
            AssignmentDestination::Inbox,
            FutureRouteAction::Set,
        ),
    ] {
        assert!(matches!(
            store.assign_activities(command).await,
            Err(StoreError::InvalidActivityAssignment(_))
        ));
        assert_eq!(state_snapshot(&pool).await, before);
    }
    assert!(matches!(
        store
            .assign_activities(ActivityAssignmentCommand::new(
                vec![999_999],
                AssignmentDestination::ProjectId(target),
                FutureRouteAction::Unchanged,
            ))
            .await,
        Err(StoreError::ActivityNotFound(999_999))
    ));
    assert!(matches!(
        store
            .assign_activities(ActivityAssignmentCommand::new(
                vec![shared_a],
                AssignmentDestination::ProjectId(999_999),
                FutureRouteAction::Unchanged,
            ))
            .await,
        Err(StoreError::ProjectNotFound(999_999))
    ));
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![shared_b, shared_a],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("mixed conversations without route action");
    assert_eq!(assignment_count(&pool, shared_origin).await, 2);
    assert_eq!(effective_project(&pool, shared_a).await, Some(target));
    assert!(!immutable_snapshot(&pool).await.is_empty());
}

async fn state_snapshot(pool: &sqlx::SqlitePool) -> Vec<Vec<String>> {
    let statements = [
        "SELECT json_array(activity_event_id, project_id, updated_at_us)
         FROM activity_project_assignments ORDER BY activity_event_id",
        "SELECT json_array(provider, provider_session_id, project_id, updated_at_us)
         FROM conversation_routes ORDER BY provider, provider_session_id",
        "SELECT json_array(id, identity, name, normalized_name) FROM projects ORDER BY id",
    ];
    let mut snapshot = Vec::new();
    for statement in statements {
        snapshot.push(
            sqlx::query_scalar(statement)
                .fetch_all(pool)
                .await
                .expect("state snapshot"),
        );
    }
    snapshot
}
