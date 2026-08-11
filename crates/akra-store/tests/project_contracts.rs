use std::{fs, path::Path};

use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{ActivityStore, RecordActivity};
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

#[tokio::test]
async fn merge_rewrites_project_references_and_preserves_every_immutable_row() {
    let directory = TempDir::new().expect("test directory");
    let database = directory.path().join("akra.sqlite");
    let store = ActivityStore::open(&database).await.expect("store");
    store.migrate().await.expect("migration");
    let pool = test_pool(&database).await;
    let source_cwd = working_directory(&directory, "source");
    let target_cwd = working_directory(&directory, "target");
    let source_activity = record(&store, &source_cwd, "source", "one", 20).await;
    let target_activity = record(&store, &target_cwd, "target", "two", 10).await;
    let source_project = project_for_activity(&pool, source_activity).await;
    let target_project = project_for_activity(&pool, target_activity).await;
    let source_origin: i64 =
        sqlx::query_scalar("SELECT origin_id FROM activity_events WHERE id = ?")
            .bind(source_activity)
            .fetch_one(&pool)
            .await
            .expect("source origin");
    sqlx::query(
        "UPDATE activity_origins SET
             routing_mode = 'shared', default_project_id = NULL, setup_state = 'confirmed'
         WHERE id = ?",
    )
    .bind(source_origin)
    .execute(&pool)
    .await
    .expect("shared source");
    sqlx::query(
        "INSERT INTO activity_project_assignments (
             activity_event_id, project_id, updated_at_us
         ) VALUES (?, ?, 1)",
    )
    .bind(source_activity)
    .bind(source_project)
    .execute(&pool)
    .await
    .expect("shared assignment");
    let target_identity_and_name: (String, String) =
        sqlx::query_as("SELECT identity, name FROM projects WHERE id = ?")
            .bind(target_project)
            .fetch_one(&pool)
            .await
            .expect("target project");
    store
        .remember_conversation_route("codex", "remembered", source_project)
        .await
        .expect("route");
    store
        .set_provider_enabled("codex", false)
        .await
        .expect("provider state");
    sqlx::query("INSERT INTO spool_receipts (spool_key, activity_event_id) VALUES ('receipt', ?)")
        .bind(source_activity)
        .execute(&pool)
        .await
        .expect("receipt");
    let nodes = store.canvas_nodes().await.expect("nodes");
    store
        .update_canvas_position(nodes[0].id, 137.0, 251.0)
        .await
        .expect("position");
    store
        .create_canvas_edge(nodes[0].id, nodes[1].id)
        .await
        .expect("edge");
    let before = immutable_snapshot(&pool).await;

    let merged = store
        .merge_projects(source_project, target_project)
        .await
        .expect("merge");

    assert_eq!(merged.id, target_project);
    assert_eq!(merged.origin_count, 1);
    assert_eq!(merged.activity_count, 2);
    assert_eq!(immutable_snapshot(&pool).await, before);
    assert_eq!(
        sqlx::query_as::<_, (String, String)>("SELECT identity, name FROM projects WHERE id = ?")
            .bind(target_project)
            .fetch_one(&pool)
            .await
            .expect("retained target"),
        target_identity_and_name
    );
    assert_eq!(reference_counts(&pool, source_project).await, (0, 0, 0, 0));
    assert_eq!(reference_counts(&pool, target_project).await, (1, 1, 1, 1));
}

#[tokio::test]
async fn merging_the_highest_project_id_never_reuses_it() {
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");
    let first = store.create_project("first").await.expect("first");
    let highest = store.create_project("highest").await.expect("highest");

    store
        .merge_projects(highest.id, first.id)
        .await
        .expect("merge highest into first");
    let later = store.create_project("later").await.expect("later");

    assert!(later.id > highest.id);
}

async fn immutable_snapshot(pool: &SqlitePool) -> Vec<Vec<String>> {
    let statements = [
        "SELECT json_array(id, provider, provider_session_id, provider_turn_id,
             project_identity, prompt, created_at, origin_id, submitted_cwd, captured_at_us,
             captured_at_provenance, first_recorded_at_us, first_recorded_at_provenance,
             global_sequence) FROM activity_events ORDER BY id",
        "SELECT json_array(id, activity_event_id, position_x, position_y)
         FROM canvas_nodes ORDER BY id",
        "SELECT json_array(id, source_node_id, target_node_id)
         FROM canvas_edges ORDER BY id",
        "SELECT json_array(provider, provider_session_id, provider_turn_id, activity_event_id)
         FROM ingest_dedupes ORDER BY provider, provider_session_id, provider_turn_id",
        "SELECT json_array(id, spool_key, activity_event_id) FROM spool_receipts ORDER BY id",
        "SELECT json_array(provider, enabled, installation_state, last_error, updated_at)
         FROM provider_integrations ORDER BY provider",
    ];
    let mut snapshot = Vec::with_capacity(statements.len());
    for statement in statements {
        snapshot.push(
            sqlx::query_scalar(statement)
                .fetch_all(pool)
                .await
                .expect("snapshot rows"),
        );
    }
    snapshot
}

async fn reference_counts(pool: &SqlitePool, project_id: i64) -> (i64, i64, i64, i64) {
    sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM projects WHERE id = ?),
             (SELECT COUNT(*) FROM activity_origins WHERE default_project_id = ?),
             (SELECT COUNT(*) FROM activity_project_assignments WHERE project_id = ?),
             (SELECT COUNT(*) FROM conversation_routes WHERE project_id = ?)",
    )
    .bind(project_id)
    .bind(project_id)
    .bind(project_id)
    .bind(project_id)
    .fetch_one(pool)
    .await
    .expect("reference counts")
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

async fn project_for_activity(pool: &SqlitePool, activity_id: i64) -> i64 {
    sqlx::query_scalar(
        "SELECT activity_origins.default_project_id
         FROM activity_events
         JOIN activity_origins ON activity_origins.id = activity_events.origin_id
         WHERE activity_events.id = ?",
    )
    .bind(activity_id)
    .fetch_one(pool)
    .await
    .expect("project assignment")
}

fn working_directory(directory: &TempDir, name: &str) -> std::path::PathBuf {
    let path = directory.path().join(name);
    fs::create_dir(&path).expect("working directory");
    path
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
