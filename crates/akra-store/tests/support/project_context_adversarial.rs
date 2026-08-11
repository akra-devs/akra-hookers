use sqlx::SqlitePool;

use super::support::{row_values, rows};

pub(crate) async fn partial_pre_v1_snapshot(pool: &SqlitePool) -> Vec<String> {
    let mut snapshot = schema_snapshot(pool).await;
    for table in ["activity_events", "canvas_nodes"] {
        snapshot.push(format!("table_info:{table}"));
        snapshot.extend(table_columns(pool, table).await);
    }
    snapshot.push("activity_events:data".to_owned());
    snapshot.extend(rows(pool, "SELECT id, provider, provider_session_id, provider_turn_id, project_identity, prompt, created_at FROM activity_events ORDER BY id", 7).await);
    snapshot.push("canvas_nodes:data".to_owned());
    snapshot.extend(
        rows(
            pool,
            "SELECT id, activity_event_id, position_x, position_y FROM canvas_nodes ORDER BY id",
            4,
        )
        .await,
    );
    snapshot
}

pub(crate) async fn schema_snapshot(pool: &SqlitePool) -> Vec<String> {
    rows(
        pool,
        "SELECT type, name, tbl_name, IFNULL(sql, '') FROM sqlite_master ORDER BY type, name",
        4,
    )
    .await
}

pub(crate) async fn table_columns(pool: &SqlitePool, table: &str) -> Vec<String> {
    sqlx::query(
        "SELECT cid, name, type, \"notnull\", dflt_value, pk
         FROM pragma_table_info(?) ORDER BY cid",
    )
    .bind(table)
    .fetch_all(pool)
    .await
    .expect("table columns")
    .into_iter()
    .map(|row| row_values(&row, 6))
    .collect()
}
