use akra_store::{
    ActivityAssignmentCommand, AssignmentDestination, FutureRouteAction, OriginRoutingCommand,
};

#[path = "support/origin_transition.rs"]
mod support;
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn selection_only_batch_unassign_create_and_same_target_noop() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "shared");
    let first = record(&store, &cwd, "conversation", "one", 1).await;
    let second = record(&store, &cwd, "conversation", "two", 2).await;
    let third = record(&store, &cwd, "conversation", "three", 3).await;
    let origin_id = origin_and_project(&pool, first).await.0;
    store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![first, second, third],
            AssignmentDestination::Inbox,
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("initial Inbox");
    let target = store.create_project("Target").await.expect("target").id;
    let immutable_before = immutable_snapshot(&pool).await;

    let assigned = store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![third, first, second, first],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("batch assignment");

    assert_eq!(assigned.activity_ids, vec![first, second, third]);
    assert_eq!(assigned.project_id, Some(target));
    assert_eq!(assignment_count(&pool, origin_id).await, 3);
    for activity_id in [first, second, third] {
        assert_eq!(effective_project(&pool, activity_id).await, Some(target));
    }
    let assignment_before: String = sqlx::query_scalar(
        "SELECT json_array(project_id, updated_at_us)
         FROM activity_project_assignments WHERE activity_event_id = ?",
    )
    .bind(first)
    .fetch_one(&pool)
    .await
    .expect("assignment");
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![first],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("same target");
    assert_eq!(
        sqlx::query_scalar::<_, String>(
            "SELECT json_array(project_id, updated_at_us)
             FROM activity_project_assignments WHERE activity_event_id = ?",
        )
        .bind(first)
        .fetch_one(&pool)
        .await
        .expect("same assignment"),
        assignment_before
    );

    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![second],
            AssignmentDestination::Inbox,
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("unassign");
    assert_eq!(effective_project(&pool, second).await, None);
    let created = store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![second],
            AssignmentDestination::NewProjectName("새 프로젝트".into()),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("create and assign");
    assert_ne!(created.project_id, Some(target));
    assert_eq!(effective_project(&pool, second).await, created.project_id);
    assert_eq!(immutable_snapshot(&pool).await, immutable_before);
}

#[tokio::test]
async fn future_route_set_replace_clear_never_reclassifies_past_activity() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "long-conversation");
    let first = record(&store, &cwd, "long", "one", 1).await;
    let second = record(&store, &cwd, "long", "two", 2).await;
    let third = record(&store, &cwd, "long", "three", 3).await;
    let origin_id = origin_and_project(&pool, first).await.0;
    store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    let project_a = store.create_project("A").await.expect("A").id;
    let project_b = store.create_project("B").await.expect("B").id;
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![first, second, third],
            AssignmentDestination::ProjectId(project_a),
            FutureRouteAction::Set,
        ))
        .await
        .expect("set A");
    let fourth = record(&store, &cwd, "long", "four", 4).await;
    assert_eq!(effective_project(&pool, fourth).await, Some(project_a));
    let fifth = record(&store, &cwd, "long", "five", 5).await;
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![fifth],
            AssignmentDestination::ProjectId(project_b),
            FutureRouteAction::Set,
        ))
        .await
        .expect("replace B");
    let sixth = record(&store, &cwd, "long", "six", 6).await;
    assert_eq!(effective_project(&pool, sixth).await, Some(project_b));
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![sixth],
            AssignmentDestination::ProjectId(project_b),
            FutureRouteAction::Clear,
        ))
        .await
        .expect("clear route");
    let seventh = record(&store, &cwd, "long", "seven", 7).await;

    for activity_id in [first, second, third, fourth] {
        assert_eq!(effective_project(&pool, activity_id).await, Some(project_a));
    }
    for activity_id in [fifth, sixth] {
        assert_eq!(effective_project(&pool, activity_id).await, Some(project_b));
    }
    assert_eq!(effective_project(&pool, seventh).await, None);
    assert_eq!(assignment_count(&pool, origin_id).await, 6);
    assert!(!immutable_snapshot(&pool).await.is_empty());
}
