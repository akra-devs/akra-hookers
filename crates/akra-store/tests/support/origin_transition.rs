use std::{fs, path::Path};

use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{ActivityStore, RecordActivity};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

pub(crate) async fn harness() -> (TempDir, ActivityStore, SqlitePool) {
    let directory = TempDir::new().expect("test directory");
    let database = directory.path().join("akra.sqlite");
    let store = ActivityStore::open(&database).await.expect("store");
    store.migrate().await.expect("migration");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(database)
                .foreign_keys(true),
        )
        .await
        .expect("test pool");
    (directory, store, pool)
}

pub(crate) async fn record(
    store: &ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), turn, None)
        .expect("event");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}

pub(crate) async fn origin_and_project(pool: &SqlitePool, activity_id: i64) -> (i64, i64) {
    sqlx::query_as(
        "SELECT activity_origins.id, activity_origins.default_project_id
         FROM activity_origins JOIN activity_events ON activity_events.origin_id = activity_origins.id
         WHERE activity_events.id = ?",
    )
    .bind(activity_id)
    .fetch_one(pool)
    .await
    .expect("origin")
}

pub(crate) async fn effective_project(pool: &SqlitePool, activity_id: i64) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT CASE WHEN activity_origins.routing_mode = 'dedicated'
                THEN activity_origins.default_project_id
                ELSE activity_project_assignments.project_id END
         FROM activity_events
         JOIN activity_origins ON activity_origins.id = activity_events.origin_id
         LEFT JOIN activity_project_assignments
           ON activity_project_assignments.activity_event_id = activity_events.id
         WHERE activity_events.id = ?",
    )
    .bind(activity_id)
    .fetch_one(pool)
    .await
    .expect("effective project")
}

pub(crate) async fn assignment_count(pool: &SqlitePool, origin_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT COUNT(*) FROM activity_project_assignments
         JOIN activity_events ON activity_events.id = activity_event_id
         WHERE activity_events.origin_id = ?",
    )
    .bind(origin_id)
    .fetch_one(pool)
    .await
    .expect("assignment count")
}

pub(crate) async fn immutable_snapshot(pool: &SqlitePool) -> Vec<Vec<String>> {
    let statements = [
        "SELECT json_array(id, provider, provider_session_id, provider_turn_id, prompt,
             origin_id, captured_at_us, global_sequence) FROM activity_events ORDER BY id",
        "SELECT json_array(id, activity_event_id, position_x, position_y)
         FROM canvas_nodes ORDER BY id",
        "SELECT json_array(id, source_node_id, target_node_id)
         FROM canvas_edges ORDER BY id",
    ];
    let mut snapshot = Vec::new();
    for statement in statements {
        snapshot.push(
            sqlx::query_scalar(statement)
                .fetch_all(pool)
                .await
                .expect("snapshot"),
        );
    }
    snapshot
}

pub(crate) fn working_directory(directory: &TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    fs::create_dir(&path).expect("working directory");
    path
}
