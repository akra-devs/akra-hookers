use akra_store::{
    ActivityAssignmentCommand, ActivityScope, ActivityTimeProvenance, AssignmentDestination,
    FutureRouteAction, OriginRoutingCommand,
};

#[path = "support/activity_summaries.rs"]
mod activity_summaries_support;
#[path = "support/origin_transition.rs"]
mod support;
use activity_summaries_support::{assert_position, find, record_legacy};
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn scopes_use_effective_projects_and_keep_global_conversation_numbers() {
    let (directory, store, pool) = harness().await;
    let dedicated_cwd = working_directory(&directory, "dedicated");
    let shared_cwd = working_directory(&directory, "shared");
    let first = record(&store, &dedicated_cwd, "same", "one", 1).await;
    let second = record(&store, &shared_cwd, "same", "two", 2).await;
    let shared_origin = origin_and_project(&pool, second).await.0;
    let target = origin_and_project(&pool, first).await.1;
    store
        .configure_origin(shared_origin, OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![second],
            AssignmentDestination::Inbox,
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("Inbox");
    let third = record(&store, &shared_cwd, "same", "three", 3).await;
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![third],
            AssignmentDestination::ProjectId(target),
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("assign target");
    assert_eq!(effective_project(&pool, first).await, Some(target));
    assert_eq!(effective_project(&pool, second).await, None);
    assert_eq!(effective_project(&pool, third).await, Some(target));
    assert_eq!(assignment_count(&pool, shared_origin).await, 1);
    assert!(!immutable_snapshot(&pool).await.is_empty());

    let all = store
        .activity_summaries(ActivityScope::All)
        .await
        .expect("all");
    let project = store
        .activity_summaries(ActivityScope::Project(target))
        .await
        .expect("project");
    let inbox = store
        .activity_summaries(ActivityScope::Inbox)
        .await
        .expect("Inbox");

    assert_eq!(
        all.iter().map(|summary| summary.id).collect::<Vec<_>>(),
        vec![first, second, third]
    );
    assert_eq!(
        project
            .iter()
            .map(|summary| {
                (
                    summary.id,
                    summary.conversation_index,
                    summary.conversation_total,
                )
            })
            .collect::<Vec<_>>(),
        vec![(first, 1, 3), (third, 3, 3)]
    );
    assert_eq!(
        inbox
            .iter()
            .map(|summary| {
                (
                    summary.id,
                    summary.conversation_index,
                    summary.conversation_total,
                )
            })
            .collect::<Vec<_>>(),
        vec![(second, 2, 3)]
    );
    assert_eq!(project[0].project.as_ref().expect("project").id, target);
    assert!(inbox[0].project.is_none());
    let value = serde_json::to_value(&all[0]).expect("summary JSON");
    let mut keys = value
        .as_object()
        .expect("summary object")
        .keys()
        .map(String::as_str)
        .collect::<Vec<_>>();
    keys.sort_unstable();
    assert_eq!(
        keys,
        [
            "activity_kind",
            "conversation_index",
            "conversation_total",
            "id",
            "project",
            "prompt",
            "prompt_summary",
            "provider",
            "result_summary_status",
            "time"
        ]
    );
    for forbidden in ["session_id", "turn_id", "cwd", "origin", "global_sequence"] {
        assert!(
            value.get(forbidden).is_none(),
            "{forbidden} must be omitted"
        );
    }
}

#[tokio::test]
async fn mixed_time_provenance_orders_one_conversation_once_before_filtering() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "mixed-time");
    let first = record(&store, &cwd, "mixed", "captured-300", 300).await;
    let second = record(&store, &cwd, "mixed", "captured-100-a", 100).await;
    let third = record(&store, &cwd, "mixed", "captured-100-b", 100).await;
    let legacy = record_legacy(&store, &cwd, "mixed", "legacy").await;
    sqlx::query("UPDATE activity_events SET first_recorded_at_us = 50 WHERE id = ?")
        .bind(legacy)
        .execute(&pool)
        .await
        .expect("older legacy timestamp");
    let origin_id = origin_and_project(&pool, first).await.0;
    let unknown: i64 = sqlx::query_scalar(
        "INSERT INTO activity_events (
             provider, provider_session_id, provider_turn_id, project_identity, prompt,
             origin_id, global_sequence
         ) VALUES ('codex', 'mixed', 'unknown', '', 'unknown', ?, 0)
         RETURNING id",
    )
    .bind(origin_id)
    .fetch_one(&pool)
    .await
    .expect("unknown time row");
    store
        .configure_origin(origin_id, OriginRoutingCommand::shared(true))
        .await
        .expect("shared origin");
    store
        .assign_activities(ActivityAssignmentCommand::new(
            vec![unknown],
            AssignmentDestination::Inbox,
            FutureRouteAction::Unchanged,
        ))
        .await
        .expect("unknown to Inbox");
    let all = store
        .activity_summaries(ActivityScope::All)
        .await
        .expect("all summaries");
    assert_eq!(all[0].id, unknown, "global sequence orders endpoint rows");
    assert_position(&all, legacy, 1, 5);
    assert_position(&all, second, 2, 5);
    assert_position(&all, third, 3, 5);
    assert_position(&all, first, 4, 5);
    assert_position(&all, unknown, 5, 5);
    assert_eq!(
        find(&all, first).time.provenance,
        ActivityTimeProvenance::Captured
    );
    assert_eq!(
        find(&all, first).time.value.as_deref(),
        Some("1970-01-01T00:00:00.0003Z")
    );
    assert_eq!(
        find(&all, legacy).time.provenance,
        ActivityTimeProvenance::LegacyRecorded
    );
    assert!(find(&all, legacy).time.value.is_some());
    assert_eq!(
        find(&all, unknown).time.provenance,
        ActivityTimeProvenance::Unknown
    );
    assert!(find(&all, unknown).time.value.is_none());
    let detail = store.activity_detail(first).await.expect("detail");
    assert_eq!(
        detail
            .conversation
            .iter()
            .map(|turn| turn.id)
            .collect::<Vec<_>>(),
        vec![legacy, second, third, first, unknown],
    );
    let project_id = find(&all, first).project.as_ref().expect("project").id;
    let project = store
        .activity_summaries(ActivityScope::Project(project_id))
        .await
        .expect("project");
    let inbox = store
        .activity_summaries(ActivityScope::Inbox)
        .await
        .expect("Inbox");
    assert_eq!(project.len(), 4);
    assert_eq!(inbox.len(), 1);
    assert_position(&project, legacy, 1, 5);
    assert_position(&inbox, unknown, 5, 5);
}
