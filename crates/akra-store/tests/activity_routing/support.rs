use std::{fs, path::Path};

use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{ActivityStore, RecordActivity};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

pub async fn harness() -> (TempDir, ActivityStore, SqlitePool) {
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

pub fn working_directory(directory: &TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    fs::create_dir(&path).expect("working directory");
    path
}

pub async fn record(
    store: &ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    prompt: &str,
    captured_at_us: i64,
) -> i64 {
    let event = event(cwd, session, turn, prompt);
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}

pub fn event(cwd: &Path, session: &str, turn: &str, prompt: &str) -> IngressEvent {
    IngressEvent::try_new(
        "codex",
        session,
        turn,
        cwd.to_string_lossy(),
        prompt,
        Some("test".into()),
    )
    .expect("event")
}

pub async fn insert_project(pool: &SqlitePool, name: &str) -> i64 {
    sqlx::query_scalar(
        "INSERT INTO projects (identity, display_path, name, normalized_name, created_at_us, updated_at_us)
         VALUES (?, '', ?, lower(?), 1, 1) RETURNING id",
    )
    .bind(format!("manual:{name}"))
    .bind(name)
    .bind(name)
    .fetch_one(pool)
    .await
    .expect("project")
}

pub async fn make_shared(pool: &SqlitePool) {
    sqlx::query(
        "UPDATE activity_origins
         SET routing_mode = 'shared', default_project_id = NULL, setup_state = 'confirmed'",
    )
    .execute(pool)
    .await
    .expect("shared origin");
}

pub async fn assignment(pool: &SqlitePool, activity_id: i64) -> Option<i64> {
    sqlx::query_scalar(
        "SELECT project_id FROM activity_project_assignments WHERE activity_event_id = ?",
    )
    .bind(activity_id)
    .fetch_optional(pool)
    .await
    .expect("assignment")
}
