use std::path::Path;

use akra_core::ingress::IngressEvent;
use akra_store::{ActivityStore, RecordActivity, StoreError};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

#[tokio::test]
async fn duplicate_ingress_creates_one_immutable_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");

    let first = record(&store, "session-1", "turn-1", "prompt")
        .await
        .expect("first event records");
    let duplicate = record(&store, "session-1", "turn-1", "prompt")
        .await
        .expect("duplicate is accepted");

    assert_eq!(first, duplicate);
    assert_eq!(store.activity_count().await.expect("count succeeds"), 1);
}

#[tokio::test]
async fn conflicting_dedupe_insert_rolls_back_the_entire_record() {
    let directory = TempDir::new().expect("database directory");
    let database = directory.path().join("store.sqlite");
    let store = ActivityStore::open(&database)
        .await
        .expect("database opens");
    store.migrate().await.expect("migrations apply");
    let pool = test_pool(&database).await;
    let first = record_at(&store, "C:\\first", "stable", "turn", "first")
        .await
        .expect("first event");
    sqlx::query(
        r#"
        CREATE TRIGGER inject_dedupe_conflict
        AFTER INSERT ON activity_events
        WHEN NEW.provider_session_id = 'conflict'
        BEGIN
            INSERT INTO ingest_dedupes(
                provider, provider_session_id, provider_turn_id, activity_event_id
            )
            VALUES(
                NEW.provider, NEW.provider_session_id, NEW.provider_turn_id, 1
            );
        END
        "#,
    )
    .execute(&pool)
    .await
    .expect("conflict trigger");

    let error = record_at(&store, "D:\\second", "conflict", "turn", "second")
        .await
        .expect_err("conflicting dedupe mapping must fail");

    assert!(
        matches!(error, StoreError::Invariant(_)),
        "unexpected error: {error:?}"
    );
    assert_eq!(first, 1);
    assert_eq!(store.activity_count().await.expect("activity count"), 1);
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM activity_origins")
            .fetch_one(&pool)
            .await
            .expect("origin count"),
        1,
        "the new origin must roll back with the activity"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM ingest_dedupes")
            .fetch_one(&pool)
            .await
            .expect("dedupe count"),
        1,
        "the conflicting trigger write must roll back"
    );
}

#[tokio::test]
async fn deleting_canvas_state_does_not_delete_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let activity_id = record(&store, "session-2", "turn-2", "keep this")
        .await
        .expect("event records");

    let canvas_node_id = store
        .create_canvas_node(activity_id)
        .await
        .expect("canvas node records");
    store
        .delete_canvas_node(canvas_node_id)
        .await
        .expect("canvas node deletes");

    assert_eq!(store.activity_count().await.expect("count succeeds"), 1);
}

#[tokio::test]
async fn canvas_graph_deletions_roll_back_edges_when_node_deletion_aborts() {
    let directory = TempDir::new().expect("test directory");
    let database = directory.path().join("akra.sqlite");
    let store = ActivityStore::open(&database).await.expect("store");
    store.migrate().await.expect("migration");
    let first = record(&store, "session", "one", "first")
        .await
        .expect("first activity");
    let second = record(&store, "session", "two", "second")
        .await
        .expect("second activity");
    let nodes = store.canvas_nodes().await.expect("nodes");
    let first_node = nodes
        .iter()
        .find(|node| node.activity_event_id == first)
        .expect("first node")
        .id;
    let second_node = nodes
        .iter()
        .find(|node| node.activity_event_id == second)
        .expect("second node")
        .id;
    store
        .create_canvas_edge(first_node, second_node)
        .await
        .expect("edge");
    let pool = test_pool(&database).await;
    sqlx::query(
        "CREATE TRIGGER abort_canvas_node_delete
         BEFORE DELETE ON canvas_nodes
         BEGIN
             SELECT RAISE(ABORT, 'injected node delete failure');
         END",
    )
    .execute(&pool)
    .await
    .expect("trigger");

    assert!(store.delete_canvas_node(first_node).await.is_err());
    assert_eq!(store.canvas_nodes().await.expect("nodes").len(), 2);
    assert_eq!(store.canvas_edges().await.expect("edges").len(), 1);

    assert!(store.clear_canvas().await.is_err());
    assert_eq!(store.canvas_nodes().await.expect("nodes").len(), 2);
    assert_eq!(store.canvas_edges().await.expect("edges").len(), 1);
}

#[tokio::test]
async fn duplicate_ingress_does_not_recreate_a_deleted_canvas_node() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let activity_id = record(&store, "session-cleared", "turn-cleared", "keep deleted")
        .await
        .expect("event records");
    let node_id = store
        .canvas_nodes()
        .await
        .expect("canvas nodes")
        .into_iter()
        .find(|node| node.activity_event_id == activity_id)
        .expect("initial canvas node")
        .id;
    store
        .delete_canvas_node(node_id)
        .await
        .expect("canvas node deletes");

    record(&store, "session-cleared", "turn-cleared", "keep deleted")
        .await
        .expect("duplicate is accepted");

    assert!(
        store.canvas_nodes().await.expect("canvas nodes").is_empty(),
        "duplicate delivery must not restore a deliberately deleted canvas node"
    );
}

#[tokio::test]
async fn canvas_positions_persist_independently_of_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let activity_id = record(&store, "session-3", "turn-3", "position me")
        .await
        .expect("event records");

    let node_id = store
        .create_canvas_node_at(activity_id, 120.0, 220.0)
        .await
        .expect("node records");

    assert_eq!(
        store
            .canvas_position(node_id)
            .await
            .expect("position loads"),
        Some((120.0, 220.0))
    );
    assert_eq!(store.activity_count().await.expect("activity remains"), 1);
}

#[tokio::test]
async fn canvas_edges_persist_independently_of_activity() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("migrations apply");
    let first = record(&store, "session-4", "turn-4", "first")
        .await
        .expect("first activity");
    let second = record(&store, "session-4", "turn-5", "second")
        .await
        .expect("second activity");
    let source = store.create_canvas_node(first).await.expect("source node");
    let target = store.create_canvas_node(second).await.expect("target node");

    store
        .create_canvas_edge(source, target)
        .await
        .expect("edge records");
    assert_eq!(store.canvas_edge_count().await.expect("edge count"), 1);
    assert_eq!(store.activity_count().await.expect("activities remain"), 2);
}

async fn record(
    store: &ActivityStore,
    session_id: &str,
    turn_id: &str,
    prompt: &str,
) -> Result<i64, StoreError> {
    record_at(store, "C:\\project", session_id, turn_id, prompt).await
}

async fn record_at(
    store: &ActivityStore,
    cwd: &str,
    session_id: &str,
    turn_id: &str,
    prompt: &str,
) -> Result<i64, StoreError> {
    let event = IngressEvent::try_new("codex", session_id, turn_id, cwd, prompt, None)
        .expect("valid fixture event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(cwd))
        .expect("fixture origin")
        .origin;
    store
        .record(RecordActivity::legacy_resolved(event, origin))
        .await
}

async fn test_pool(path: &Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .foreign_keys(true),
        )
        .await
        .expect("test pool")
}
