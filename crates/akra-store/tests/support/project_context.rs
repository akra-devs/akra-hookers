use std::path::Path;

use sqlx::{
    Row, SqlitePool, ValueRef,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};

pub(crate) async fn seed_legacy_fixture(pool: &SqlitePool) {
    sqlx::raw_sql(
        r#"
        CREATE TABLE projects (id INTEGER PRIMARY KEY, identity TEXT NOT NULL UNIQUE, display_path TEXT NOT NULL DEFAULT '');
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
        CREATE TABLE canvas_edges (
          id INTEGER PRIMARY KEY, source_node_id INTEGER NOT NULL, target_node_id INTEGER NOT NULL,
          FOREIGN KEY(source_node_id) REFERENCES canvas_nodes(id), FOREIGN KEY(target_node_id) REFERENCES canvas_nodes(id)
        );
        CREATE TABLE ingest_dedupes (
          provider TEXT NOT NULL, provider_session_id TEXT NOT NULL, provider_turn_id TEXT NOT NULL,
          activity_event_id INTEGER NOT NULL, PRIMARY KEY(provider, provider_session_id, provider_turn_id),
          FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
        );
        CREATE TABLE provider_integrations (
          provider TEXT PRIMARY KEY, enabled INTEGER NOT NULL, installation_state TEXT NOT NULL,
          last_error TEXT, updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        CREATE TABLE spool_receipts (
          id INTEGER PRIMARY KEY, spool_key TEXT NOT NULL UNIQUE, activity_event_id INTEGER NOT NULL,
          FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
        );
        INSERT INTO projects VALUES (10, 'identity-a', 'C:\repos\same');
        INSERT INTO projects VALUES (20, 'identity-b', 'D:\other\same');
        INSERT INTO projects VALUES (30, 'orphan', 'C:\stale\unused');
        INSERT INTO activity_events VALUES (101, 'codex', 'session-a', 'turn-1', 'identity-a', 'first prompt', '2025-01-01 00:00:00');
        INSERT INTO activity_events VALUES (102, 'codex', 'session-a', 'turn-2', 'missing-identity', 'missing project prompt', 'not-a-date');
        INSERT INTO activity_events VALUES (103, 'claude', 'session-b', 'turn-1', 'identity-b', 'node intentionally absent', '2025-01-02T00:00:00Z');
        INSERT INTO canvas_nodes VALUES (201, 101, 12.5, -7.25);
        INSERT INTO canvas_nodes VALUES (202, 102, 640.0, 480.0);
        INSERT INTO canvas_edges VALUES (301, 201, 202);
        INSERT INTO canvas_edges VALUES (302, 201, 202);
        INSERT INTO canvas_edges VALUES (303, 202, 201);
        INSERT INTO ingest_dedupes VALUES ('codex', 'session-a', 'turn-1', 101);
        INSERT INTO ingest_dedupes VALUES ('codex', 'session-a', 'turn-2', 102);
        INSERT INTO ingest_dedupes VALUES ('claude', 'session-b', 'turn-1', 103);
        INSERT INTO provider_integrations VALUES ('codex', 0, 'broken', 'intentional error', '2025-01-03 04:05:06');
        INSERT INTO spool_receipts VALUES (401, 'receipt-a', 101);
        "#,
    )
    .execute(pool)
    .await
    .expect("legacy fixture");
}

pub(crate) async fn legacy_snapshot(pool: &SqlitePool) -> Vec<String> {
    let mut snapshot = Vec::new();
    for (sql, columns) in [
        (
            "SELECT id, provider, provider_session_id, provider_turn_id, project_identity, prompt, created_at FROM activity_events ORDER BY id",
            7,
        ),
        (
            "SELECT id, activity_event_id, position_x, position_y FROM canvas_nodes ORDER BY id",
            4,
        ),
        (
            "SELECT id, source_node_id, target_node_id FROM canvas_edges ORDER BY id",
            3,
        ),
        (
            "SELECT provider, provider_session_id, provider_turn_id, activity_event_id FROM ingest_dedupes ORDER BY provider, provider_session_id, provider_turn_id",
            4,
        ),
        (
            "SELECT provider, enabled, installation_state, last_error, updated_at FROM provider_integrations ORDER BY provider",
            5,
        ),
        (
            "SELECT id, spool_key, activity_event_id FROM spool_receipts ORDER BY id",
            3,
        ),
    ] {
        snapshot.extend(rows(pool, sql, columns).await);
    }
    snapshot
}

pub(crate) async fn full_snapshot(pool: &SqlitePool) -> Vec<String> {
    let mut snapshot = legacy_snapshot(pool).await;
    for (sql, columns) in [
        (
            "SELECT id, identity, display_path, name, normalized_name, created_at_us, updated_at_us FROM projects ORDER BY id",
            7,
        ),
        (
            "SELECT id, identity, kind, resolution_source, display_path, routing_mode, default_project_id, setup_state, created_at_us, updated_at_us FROM activity_origins ORDER BY id",
            10,
        ),
        (
            "SELECT id, origin_id, submitted_cwd, captured_at_us, captured_at_provenance, first_recorded_at_us, first_recorded_at_provenance, global_sequence FROM activity_events ORDER BY id",
            8,
        ),
        (
            "SELECT version, applied_at_us FROM schema_migrations ORDER BY version",
            2,
        ),
    ] {
        snapshot.extend(rows(pool, sql, columns).await);
    }
    snapshot
}

pub(crate) async fn rows(pool: &SqlitePool, sql: &str, columns: usize) -> Vec<String> {
    sqlx::query(sql)
        .fetch_all(pool)
        .await
        .expect("snapshot query")
        .into_iter()
        .map(|row| row_values(&row, columns))
        .collect()
}

pub(crate) fn row_values(row: &sqlx::sqlite::SqliteRow, columns: usize) -> String {
    (0..columns)
        .map(|index| {
            if row
                .try_get_raw(index)
                .map(|value| value.is_null())
                .unwrap_or(false)
            {
                return "NULL".to_owned();
            }
            if let Ok(value) = row.try_get::<String, _>(index) {
                value
            } else if let Ok(value) = row.try_get::<i64, _>(index) {
                value.to_string()
            } else if let Ok(value) = row.try_get::<f64, _>(index) {
                value.to_string()
            } else {
                "NULL".to_owned()
            }
        })
        .collect::<Vec<_>>()
        .join("|")
}

pub(crate) async fn scalar(pool: &SqlitePool, sql: &str) -> i64 {
    sqlx::query_scalar(sql)
        .fetch_one(pool)
        .await
        .expect("scalar")
}

pub(crate) async fn fixture_pool(path: &Path, foreign_keys: bool) -> SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .create_if_missing(true)
                .foreign_keys(foreign_keys),
        )
        .await
        .expect("fixture database")
}

pub(crate) async fn open_pool(path: &Path) -> SqlitePool {
    fixture_pool(path, true).await
}
