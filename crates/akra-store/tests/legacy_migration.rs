use akra_store::ActivityStore;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use tempfile::TempDir;

#[tokio::test]
async fn migrates_pre_project_activity_data_into_visible_legacy_project() {
    let directory = TempDir::new().expect("database directory");
    let database_path = directory.path().join("legacy.sqlite");
    let pool = legacy_pool(&database_path).await;
    sqlx::raw_sql(
        "
        CREATE TABLE projects (
          id INTEGER PRIMARY KEY,
          identity TEXT NOT NULL UNIQUE
        );
        CREATE TABLE activity_events (
          id INTEGER PRIMARY KEY,
          provider TEXT NOT NULL,
          provider_session_id TEXT NOT NULL,
          provider_turn_id TEXT NOT NULL,
          prompt TEXT NOT NULL,
          UNIQUE(provider, provider_session_id, provider_turn_id)
        );
        INSERT INTO activity_events (
          provider, provider_session_id, provider_turn_id, prompt
        ) VALUES ('codex', 'legacy-session', 'legacy-turn', 'preserve me');
        ",
    )
    .execute(&pool)
    .await
    .expect("legacy schema");
    drop(pool);

    let store = ActivityStore::open(&database_path).await.expect("store");
    store.migrate().await.expect("migration");

    let projects = store.projects().await.expect("projects");
    assert!(projects.iter().any(|project| {
        project.identity == "__legacy__" && project.display_path == "Legacy activity"
    }));
    let activities = store
        .activities_for_project(Some("__legacy__"))
        .await
        .expect("legacy activities");
    assert_eq!(activities.len(), 1);
    assert_eq!(activities[0].prompt, "preserve me");
}

#[tokio::test]
async fn migration_removes_unassociated_legacy_project_rows() {
    let directory = TempDir::new().expect("database directory");
    let database_path = directory.path().join("legacy-orphans.sqlite");
    let pool = legacy_pool(&database_path).await;
    sqlx::raw_sql(
        "
        CREATE TABLE projects (
          id INTEGER PRIMARY KEY,
          identity TEXT NOT NULL UNIQUE
        );
        CREATE TABLE activity_events (
          id INTEGER PRIMARY KEY,
          provider TEXT NOT NULL,
          provider_session_id TEXT NOT NULL,
          provider_turn_id TEXT NOT NULL,
          prompt TEXT NOT NULL,
          UNIQUE(provider, provider_session_id, provider_turn_id)
        );
        INSERT INTO projects (identity) VALUES ('stale-project');
        INSERT INTO activity_events (
          provider, provider_session_id, provider_turn_id, prompt
        ) VALUES ('codex', 'legacy-session', 'legacy-turn', 'preserve me');
        ",
    )
    .execute(&pool)
    .await
    .expect("legacy schema");
    drop(pool);

    let store = ActivityStore::open(&database_path).await.expect("store");
    store.migrate().await.expect("migration");

    let projects = store.projects().await.expect("projects");
    assert!(
        projects
            .iter()
            .all(|project| project.identity != "stale-project"),
        "legacy project rows with no associated activity must not become blank filters"
    );
}

async fn legacy_pool(path: &std::path::Path) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true),
        )
        .await
        .expect("legacy database")
}
