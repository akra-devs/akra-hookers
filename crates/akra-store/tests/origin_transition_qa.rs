use akra_store::{OriginRoutingCommand, ProjectDestination};

#[path = "support/origin_transition.rs"]
mod support;
use support::{
    assignment_count, effective_project, harness, immutable_snapshot, origin_and_project, record,
    working_directory,
};

#[tokio::test]
async fn unrelated_origins_share_one_project_across_mode_round_trip() {
    let (directory, store, pool) = harness().await;
    let first_cwd = working_directory(&directory, "first-work");
    let second_cwd = working_directory(&directory, "second-work");
    let first = record(&store, &first_cwd, "first", "one", 1).await;
    let second = record(&store, &second_cwd, "second", "one", 2).await;
    let first_origin = origin_and_project(&pool, first).await.0;
    let second_origin = origin_and_project(&pool, second).await.0;
    let target = store.create_project("Together").await.expect("target").id;
    for origin_id in [first_origin, second_origin] {
        store
            .configure_origin(
                origin_id,
                OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(target), true),
            )
            .await
            .expect("connect origin");
    }
    let before = immutable_snapshot(&pool).await;
    assert_eq!(effective_project(&pool, first).await, Some(target));
    assert_eq!(assignment_count(&pool, first_origin).await, 0);
    assert_eq!(project_counts(&store, target).await, (2, 2));

    store
        .configure_origin(first_origin, OriginRoutingCommand::shared(true))
        .await
        .expect("shared");
    assert_eq!(assignment_count(&pool, first_origin).await, 1);
    store
        .configure_origin(
            first_origin,
            OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(target), true),
        )
        .await
        .expect("dedicated");

    assert_eq!(assignment_count(&pool, first_origin).await, 0);
    assert_eq!(effective_project(&pool, second).await, Some(target));
    assert_eq!(project_counts(&store, target).await, (2, 2));
    assert_eq!(immutable_snapshot(&pool).await, before);
}

#[tokio::test]
async fn shared_route_survives_until_the_last_shared_origin_leaves() {
    let (directory, store, pool) = harness().await;
    let first_cwd = working_directory(&directory, "first-shared");
    let second_cwd = working_directory(&directory, "second-shared");
    let first = record(&store, &first_cwd, "same", "one", 1).await;
    let second = record(&store, &second_cwd, "same", "two", 2).await;
    let (first_origin, first_project) = origin_and_project(&pool, first).await;
    let (second_origin, second_project) = origin_and_project(&pool, second).await;
    for origin_id in [first_origin, second_origin] {
        store
            .configure_origin(origin_id, OriginRoutingCommand::shared(true))
            .await
            .expect("shared origin");
    }
    store
        .remember_conversation_route("codex", "same", first_project)
        .await
        .expect("shared route");
    let before = immutable_snapshot(&pool).await;

    store
        .configure_origin(
            first_origin,
            OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(first_project), true),
        )
        .await
        .expect("first dedicated");

    assert_eq!(route_count(&pool).await, 1);
    assert_eq!(immutable_snapshot(&pool).await, before);
    let future = record(&store, &second_cwd, "same", "future", 3).await;
    assert_eq!(effective_project(&pool, future).await, Some(first_project));
    assert_eq!(assignment_count(&pool, second_origin).await, 2);
    store
        .configure_origin(
            second_origin,
            OriginRoutingCommand::dedicated(ProjectDestination::ProjectId(second_project), true),
        )
        .await
        .expect("last dedicated");
    assert_eq!(route_count(&pool).await, 0);
}

async fn project_counts(store: &akra_store::ActivityStore, project_id: i64) -> (i64, i64) {
    store
        .projects()
        .await
        .expect("projects")
        .into_iter()
        .find(|project| project.id == project_id)
        .map(|project| (project.origin_count, project.activity_count))
        .expect("target summary")
}

async fn route_count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM conversation_routes
         WHERE provider = 'codex' AND provider_session_id = 'same'",
    )
    .fetch_one(pool)
    .await
    .expect("route count")
}
