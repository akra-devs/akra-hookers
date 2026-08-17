#[allow(dead_code)]
#[path = "support/origin_transition.rs"]
mod support;

use akra_store::{ActivityScope, StoreError};
use support::{harness, record, working_directory};

#[tokio::test]
async fn deleting_an_activity_tombstones_it_and_removes_every_live_projection() {
    let (directory, store, pool) = harness().await;
    let cwd = working_directory(&directory, "activity-delete");
    let first = record(&store, &cwd, "conversation", "first", 100).await;
    let deleted = record(&store, &cwd, "conversation", "deleted", 200).await;
    let third = record(&store, &cwd, "conversation", "third", 300).await;
    let nodes = store.canvas_nodes().await.expect("nodes");
    let deleted_node = nodes
        .iter()
        .find(|node| node.activity_event_id == deleted)
        .expect("deleted node")
        .id;
    let third_node = nodes
        .iter()
        .find(|node| node.activity_event_id == third)
        .expect("third node")
        .id;
    store
        .create_canvas_edge(deleted_node, third_node)
        .await
        .expect("edge");
    let revision = store.canvas_revision().await.expect("revision");

    store
        .delete_activity(deleted)
        .await
        .expect("delete activity");

    let tombstone: (Option<i64>, String) =
        sqlx::query_as("SELECT deleted_at_us, prompt FROM activity_events WHERE id = ?")
            .bind(deleted)
            .fetch_one(&pool)
            .await
            .expect("tombstone");
    assert!(tombstone.0.is_some());
    assert_eq!(tombstone.1, "deleted");
    assert_eq!(store.activity_count().await.expect("active count"), 2);
    assert!(matches!(
        store.activity_detail(deleted).await,
        Err(StoreError::ActivityNotFound(id)) if id == deleted
    ));
    assert!(matches!(
        store.delete_activity(deleted).await,
        Err(StoreError::ActivityNotFound(id)) if id == deleted
    ));

    let summaries = store
        .activity_summaries(ActivityScope::All)
        .await
        .expect("summaries");
    assert_eq!(
        summaries
            .iter()
            .map(|summary| summary.id)
            .collect::<Vec<_>>(),
        vec![first, third]
    );
    assert_eq!(
        summaries
            .iter()
            .map(|summary| (summary.conversation_index, summary.conversation_total))
            .collect::<Vec<_>>(),
        vec![(1, 2), (2, 2)]
    );
    let detail = store
        .activity_detail(third)
        .await
        .expect("remaining detail");
    assert_eq!(detail.conversation_index, 2);
    assert_eq!(detail.conversation_total, 2);
    assert_eq!(
        detail
            .conversation
            .iter()
            .map(|turn| turn.id)
            .collect::<Vec<_>>(),
        vec![first, third]
    );
    assert_eq!(detail.origin.activity_count, 2);

    let project = store.projects().await.expect("projects").remove(0);
    assert_eq!(project.activity_count, 2);
    let origin = store.origins().await.expect("origins").remove(0);
    assert_eq!(origin.activity_count, 2);
    assert_eq!(origin.conversation_count, 1);
    assert_eq!(
        store
            .canvas_nodes()
            .await
            .expect("active canvas")
            .iter()
            .map(|node| node.activity_event_id)
            .collect::<Vec<_>>(),
        vec![first, third]
    );
    assert!(store.canvas_edges().await.expect("edges").is_empty());
    let node_tombstone: Option<i64> =
        sqlx::query_scalar("SELECT deleted_at_us FROM canvas_nodes WHERE id = ?")
            .bind(deleted_node)
            .fetch_one(&pool)
            .await
            .expect("node tombstone");
    assert!(node_tombstone.is_some());
    assert!(store.canvas_revision().await.expect("new revision") > revision);

    let replayed = record(&store, &cwd, "conversation", "deleted", 400).await;
    assert_eq!(replayed, deleted);
    assert_eq!(store.activity_count().await.expect("still deleted"), 2);
}

#[tokio::test]
async fn offset_pages_skip_deleted_conversation_turns() {
    let (directory, store, _) = harness().await;
    let cwd = working_directory(&directory, "offset-pages");
    let first = record(&store, &cwd, "conversation", "first", 100).await;
    let second = record(&store, &cwd, "conversation", "second", 200).await;
    let third = record(&store, &cwd, "conversation", "third", 300).await;
    store.delete_activity(second).await.expect("delete second");

    let page = store
        .activity_detail_offset_page_filtered(third, 1, 1, akra_store::ActivityKindFilter::ALL)
        .await
        .expect("offset page");
    assert_eq!(page.conversation_index, 2);
    assert_eq!(page.conversation_total, 2);
    assert_eq!(
        page.conversation
            .iter()
            .map(|turn| turn.id)
            .collect::<Vec<_>>(),
        vec![third]
    );
    assert!(!page.conversation_has_more);

    let first_page = store
        .activity_detail_offset_page_filtered(first, 0, 1, akra_store::ActivityKindFilter::ALL)
        .await
        .expect("first page");
    assert_eq!(first_page.conversation[0].id, first);
    assert!(first_page.conversation_has_more);
}
