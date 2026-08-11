use std::{fs, path::Path, sync::Arc};

use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{ActivityStore, RecordActivity};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;
use tokio::sync::oneshot;

#[tokio::test]
async fn signaled_concurrent_operations_classify_by_transaction_commit_order() {
    let directory = TempDir::new().expect("test directory");
    let cwd = directory.path().join("shared-root");
    fs::create_dir(&cwd).expect("working directory");
    let database = directory.path().join("akra.sqlite");
    let store = Arc::new(ActivityStore::open(&database).await.expect("store"));
    store.migrate().await.expect("migration");
    let pool = test_pool(&database).await;
    record(&store, &cwd, "setup", "setup", 1).await;
    sqlx::query(
        "UPDATE activity_origins
         SET routing_mode = 'shared', default_project_id = NULL, setup_state = 'confirmed'",
    )
    .execute(&pool)
    .await
    .expect("shared origin");
    let target: i64 = sqlx::query_scalar(
        "INSERT INTO projects (
             identity, display_path, name, normalized_name, created_at_us, updated_at_us
         ) VALUES ('manual:target', '', 'Target', 'target', 1, 1)
         RETURNING id",
    )
    .fetch_one(&pool)
    .await
    .expect("target project");
    let (before_committed_tx, before_committed_rx) = oneshot::channel();
    let (route_committed_tx, route_committed_rx) = oneshot::channel();

    let before_store = Arc::clone(&store);
    let before_cwd = cwd.clone();
    let before = tokio::spawn(async move {
        let id = record(&before_store, &before_cwd, "future", "before", 300).await;
        before_committed_tx.send(()).expect("before signal");
        id
    });
    let route_store = Arc::clone(&store);
    let route = tokio::spawn(async move {
        before_committed_rx.await.expect("before committed");
        route_store
            .remember_conversation_route("codex", "future", target)
            .await
            .expect("route commit");
        route_committed_tx.send(()).expect("route signal");
    });
    let after_store = Arc::clone(&store);
    let after_cwd = cwd.clone();
    let after = tokio::spawn(async move {
        route_committed_rx.await.expect("route committed");
        record(&after_store, &after_cwd, "future", "after", 100).await
    });

    let before_id = before.await.expect("before task");
    route.await.expect("route task");
    let after_id = after.await.expect("after task");

    assert_eq!(assignment(&pool, before_id).await, None);
    assert_eq!(assignment(&pool, after_id).await, Some(target));
    let rows: Vec<(String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT provider_turn_id, global_sequence, captured_at_us
         FROM activity_events WHERE provider_session_id = 'future'
         ORDER BY global_sequence",
    )
    .fetch_all(&pool)
    .await
    .expect("ordered events");
    assert_eq!(
        rows,
        vec![
            ("before".to_owned(), 2, Some(300)),
            ("after".to_owned(), 3, Some(100)),
        ]
    );
}

async fn record(
    store: &ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new(
        "codex",
        session,
        turn,
        cwd.to_string_lossy(),
        turn,
        Some("test".into()),
    )
    .expect("event");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}

async fn assignment(pool: &SqlitePool, activity_id: i64) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT project_id FROM activity_project_assignments WHERE activity_event_id = ?",
    )
    .bind(activity_id)
    .fetch_optional(pool)
    .await
    .expect("assignment")
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
