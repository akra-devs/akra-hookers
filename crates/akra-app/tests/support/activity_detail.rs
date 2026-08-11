use std::{path::Path, sync::Arc};

use akra_core::ingress::IngressEvent;
use akra_store::RecordActivity;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

pub(crate) async fn record_captured(
    store: &akra_store::ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    prompt: &str,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), prompt, None)
        .expect("event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}

pub(crate) async fn legacy_app() -> (TempDir, axum::Router) {
    let directory = TempDir::new().expect("legacy directory");
    let path = directory.path().join("legacy-detail.sqlite");
    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(&path)
                .create_if_missing(true)
                .foreign_keys(true),
        )
        .await
        .expect("legacy pool");
    sqlx::raw_sql(include_str!(
        "../../../akra-store/migrations/0001_initial.sql"
    ))
    .execute(&pool)
    .await
    .expect("v1 schema");
    sqlx::raw_sql(
        "INSERT INTO projects (id, identity, display_path)
         VALUES (7, 'legacy-origin', 'C:\\detected\\legacy');
         INSERT INTO activity_events (
             id, provider, provider_session_id, provider_turn_id,
             project_identity, prompt, created_at
         ) VALUES (
             41, 'codex', 'legacy-session', 'legacy-turn',
             'legacy-origin', 'legacy full prompt', '2025-01-02 03:04:05'
         );",
    )
    .execute(&pool)
    .await
    .expect("legacy fixture");
    drop(pool);
    let store = Arc::new(
        akra_store::ActivityStore::open(&path)
            .await
            .expect("legacy store"),
    );
    store.migrate().await.expect("legacy migration");
    (directory, akra_app::http::app("fixture-token", store))
}
