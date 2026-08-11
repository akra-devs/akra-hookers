use akra_store::ActivityStore;
use tempfile::TempDir;

#[path = "support/project_context.rs"]
mod support;
use support::*;

#[path = "support/project_context_adversarial.rs"]
mod adversarial_support;
use adversarial_support::*;

#[tokio::test]
async fn migrated_projects_reject_blank_names() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("project-name-check.sqlite");
    let pool = fixture_pool(&path, false).await;
    seed_legacy_fixture(&pool).await;
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    store.migrate().await.expect("migration succeeds");
    drop(store);

    let pool = open_pool(&path).await;
    assert!(
        sqlx::query(
            "INSERT INTO projects (id, identity, display_path, name, normalized_name, created_at_us, updated_at_us) VALUES (99, 'blank-name', '', '   ', 'blank-name', 1, 1)",
        )
        .execute(&pool)
        .await
        .is_err(),
        "whitespace-only project names must be rejected"
    );
    assert!(
        sqlx::query("UPDATE projects SET name = '  ' WHERE id = 10")
            .execute(&pool)
            .await
            .is_err(),
        "updates to whitespace-only project names must be rejected"
    );
}

#[tokio::test]
async fn integrity_failure_rolls_back_every_v2_change() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("rollback.sqlite");
    let pool = fixture_pool(&path, false).await;
    seed_legacy_fixture(&pool).await;
    sqlx::query("INSERT INTO canvas_nodes (id, activity_event_id, position_x, position_y) VALUES (999, 9999, 1, 2)")
        .execute(&pool)
        .await
        .expect("inject invalid legacy foreign key");
    let before = legacy_snapshot(&pool).await;
    let before_projects = rows(
        &pool,
        "SELECT id, identity, display_path FROM projects ORDER BY id",
        3,
    )
    .await;
    let before_project_columns = table_columns(&pool, "projects").await;
    let before_activity_columns = table_columns(&pool, "activity_events").await;
    let before_schema = schema_snapshot(&pool).await;
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    let error = store
        .migrate()
        .await
        .expect_err("foreign-key defect must fail");
    assert!(error.to_string().contains("foreign key check"), "{error}");
    drop(store);

    let pool = open_pool(&path).await;
    assert_eq!(legacy_snapshot(&pool).await, before);
    assert_eq!(
        rows(
            &pool,
            "SELECT id, identity, display_path FROM projects ORDER BY id",
            3,
        )
        .await,
        before_projects
    );
    assert_eq!(
        table_columns(&pool, "projects").await,
        before_project_columns
    );
    assert_eq!(
        table_columns(&pool, "activity_events").await,
        before_activity_columns
    );
    assert_eq!(schema_snapshot(&pool).await, before_schema);
    for table in [
        "activity_origins",
        "activity_project_assignments",
        "conversation_routes",
        "schema_migrations",
    ] {
        assert_eq!(
            scalar(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
                ),
            )
            .await,
            0,
            "{table} must not persist after the failed migration"
        );
    }
}

#[tokio::test]
async fn partial_pre_v1_failure_rolls_back_all_schema_changes() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("partial-pre-v1-rollback.sqlite");
    let pool = fixture_pool(&path, false).await;
    sqlx::raw_sql(
        r#"
        CREATE TABLE activity_events (
          id INTEGER PRIMARY KEY, provider TEXT NOT NULL, provider_session_id TEXT NOT NULL,
          provider_turn_id TEXT NOT NULL, project_identity TEXT NOT NULL DEFAULT '', prompt TEXT NOT NULL,
          created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
          UNIQUE(provider, provider_session_id, provider_turn_id)
        );
        CREATE TABLE canvas_nodes (
          id INTEGER PRIMARY KEY, activity_event_id INTEGER NOT NULL, position_x REAL NOT NULL DEFAULT 64,
          position_y REAL NOT NULL DEFAULT 64, FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
        );
        INSERT INTO activity_events VALUES (1, 'codex', 'session', 'turn', 'identity', 'prompt', '2025-01-01 00:00:00');
        INSERT INTO canvas_nodes VALUES (2, 999, 12.5, -7.25);
        "#,
    )
    .execute(&pool)
    .await
    .expect("partial legacy fixture");
    let before = partial_pre_v1_snapshot(&pool).await;
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    let error = store
        .migrate()
        .await
        .expect_err("foreign-key defect must fail");
    assert!(error.to_string().contains("foreign key check"), "{error}");
    drop(store);

    let pool = open_pool(&path).await;
    assert_eq!(partial_pre_v1_snapshot(&pool).await, before);
    for table in [
        "projects",
        "canvas_edges",
        "ingest_dedupes",
        "provider_integrations",
        "spool_receipts",
        "schema_migrations",
        "activity_origins",
        "activity_project_assignments",
        "conversation_routes",
    ] {
        assert_eq!(
            scalar(
                &pool,
                &format!(
                    "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = '{table}'"
                ),
            )
            .await,
            0,
            "{table} must not persist after the failed migration"
        );
    }
}

#[tokio::test]
async fn migration_preserves_blank_legacy_project_identity() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("blank-legacy-identity.sqlite");
    let pool = fixture_pool(&path, false).await;
    seed_legacy_fixture(&pool).await;
    sqlx::query(
        "INSERT INTO activity_events
         VALUES (104, 'codex', 'session-blank', 'turn-blank', '', 'blank identity', '2025-01-03')",
    )
    .execute(&pool)
    .await
    .expect("blank legacy identity row");
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    store.migrate().await.expect("migration succeeds");
    drop(store);

    let pool = open_pool(&path).await;
    assert!(
        full_snapshot(&pool)
            .await
            .iter()
            .any(|row| row.contains("blank identity")),
        "immutable blank-identity activity remains in the full snapshot"
    );
    let (identity, origin_id): (String, Option<i64>) =
        sqlx::query_as("SELECT project_identity, origin_id FROM activity_events WHERE id = 104")
            .fetch_one(&pool)
            .await
            .expect("migrated blank identity row");
    assert_eq!(identity, "", "deprecated identity remains byte-for-byte");
    assert!(
        origin_id.is_some(),
        "blank identity still maps to an origin"
    );
}

#[tokio::test]
async fn migration_normalizes_unicode_names_before_suffixing() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("unicode-project-names.sqlite");
    let pool = fixture_pool(&path, false).await;
    seed_legacy_fixture(&pool).await;
    sqlx::query("UPDATE projects SET display_path = 'C:\\repos\\ＦＯＯ' WHERE id = 10")
        .execute(&pool)
        .await
        .expect("full-width project path");
    sqlx::query("UPDATE projects SET display_path = 'D:\\other\\foo' WHERE id = 20")
        .execute(&pool)
        .await
        .expect("ASCII project path");
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    store.migrate().await.expect("migration succeeds");
    drop(store);

    let pool = open_pool(&path).await;
    let names: Vec<(String, String)> =
        sqlx::query_as("SELECT name, normalized_name FROM projects ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("migrated project names");
    assert_eq!(
        names,
        vec![
            ("ＦＯＯ".to_owned(), "foo".to_owned()),
            ("foo (2)".to_owned(), "foo (2)".to_owned()),
            ("missing-identity".to_owned(), "missing-identity".to_owned(),),
        ]
    );
}
