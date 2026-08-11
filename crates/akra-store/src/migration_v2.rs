use crate::StoreError;
use sqlx::SqliteConnection;

pub(crate) const V2_COLUMNS: [(&str, &str, &str); 14] = [
    (
        "projects",
        "display_path",
        "ALTER TABLE projects ADD COLUMN display_path TEXT NOT NULL DEFAULT ''",
    ),
    (
        "projects",
        "name",
        "ALTER TABLE projects ADD COLUMN name TEXT NOT NULL DEFAULT ''",
    ),
    (
        "projects",
        "normalized_name",
        "ALTER TABLE projects ADD COLUMN normalized_name TEXT NOT NULL DEFAULT ''",
    ),
    (
        "projects",
        "created_at_us",
        "ALTER TABLE projects ADD COLUMN created_at_us INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "projects",
        "updated_at_us",
        "ALTER TABLE projects ADD COLUMN updated_at_us INTEGER NOT NULL DEFAULT 0",
    ),
    (
        "activity_events",
        "project_identity",
        "ALTER TABLE activity_events ADD COLUMN project_identity TEXT NOT NULL DEFAULT ''",
    ),
    (
        "activity_events",
        "origin_id",
        "ALTER TABLE activity_events ADD COLUMN origin_id INTEGER REFERENCES activity_origins(id)",
    ),
    (
        "activity_events",
        "submitted_cwd",
        "ALTER TABLE activity_events ADD COLUMN submitted_cwd TEXT",
    ),
    (
        "activity_events",
        "captured_at_us",
        "ALTER TABLE activity_events ADD COLUMN captured_at_us INTEGER",
    ),
    (
        "activity_events",
        "captured_at_provenance",
        "ALTER TABLE activity_events ADD COLUMN captured_at_provenance TEXT CHECK (captured_at_provenance IN ('captured'))",
    ),
    (
        "activity_events",
        "created_at",
        "ALTER TABLE activity_events ADD COLUMN created_at TEXT",
    ),
    (
        "activity_events",
        "first_recorded_at_us",
        "ALTER TABLE activity_events ADD COLUMN first_recorded_at_us INTEGER",
    ),
    (
        "activity_events",
        "first_recorded_at_provenance",
        "ALTER TABLE activity_events ADD COLUMN first_recorded_at_provenance TEXT CHECK (first_recorded_at_provenance IN ('captured', 'legacy_recorded'))",
    ),
    (
        "activity_events",
        "global_sequence",
        "ALTER TABLE activity_events ADD COLUMN global_sequence INTEGER",
    ),
];

pub(crate) async fn add_column_if_missing(
    connection: &mut SqliteConnection,
    table: &str,
    column: &str,
    statement: &str,
) -> Result<(), StoreError> {
    let exists =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?")
            .bind(table)
            .bind(column)
            .fetch_one(&mut *connection)
            .await?;
    if exists == 0 {
        sqlx::query(statement).execute(&mut *connection).await?;
    }
    Ok(())
}

pub(crate) async fn rebuild_projects_with_name_check(
    connection: &mut SqliteConnection,
) -> Result<(), StoreError> {
    sqlx::raw_sql(
        "CREATE TABLE projects_replacement (
            id INTEGER PRIMARY KEY,
            identity TEXT NOT NULL UNIQUE,
            display_path TEXT NOT NULL DEFAULT '',
            name TEXT NOT NULL,
            normalized_name TEXT NOT NULL UNIQUE,
            created_at_us INTEGER NOT NULL,
            updated_at_us INTEGER NOT NULL,
            CHECK (length(trim(name)) > 0)
        );
        INSERT INTO projects_replacement
            (id, identity, display_path, name, normalized_name, created_at_us, updated_at_us)
        SELECT id, identity, display_path, name, normalized_name, created_at_us, updated_at_us
        FROM projects;
        DROP TABLE projects;
        ALTER TABLE projects_replacement RENAME TO projects;",
    )
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) fn suggested_name(display_path: &str, identity: &str) -> String {
    display_path
        .rsplit(['/', '\\'])
        .find(|part| !part.is_empty())
        .unwrap_or(identity)
        .to_owned()
}
