use crate::{
    ActivityStore, ProjectNames, StoreError,
    migration_v2::{
        V2_COLUMNS, add_column_if_missing, rebuild_projects_with_name_check, suggested_name,
    },
};
use sqlx::Row;

const LEGACY_BLANK_IDENTITY: &str = "__legacy__";

impl ActivityStore {
    pub async fn migrate(&self) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                applied_at_us INTEGER NOT NULL
            )",
        )
        .execute(&mut *transaction)
        .await?;
        if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 2")
            .fetch_one(&mut *transaction)
            .await?
            != 0
        {
            crate::migration_v3::apply(&mut transaction).await?;
            crate::migration_v4::apply(&mut transaction).await?;
            crate::migration_v5::apply(&mut transaction).await?;
            crate::migration_v6::apply(&mut transaction).await?;
            crate::migration_v7::apply(&mut transaction).await?;
            crate::migration_v8::apply(&mut transaction).await?;
            crate::migration_v9::apply(&mut transaction).await?;
            crate::migration_v10::apply(&mut transaction).await?;
            crate::migration_v11::apply(&mut transaction).await?;
            crate::migration_v12::apply(&mut transaction).await?;
            crate::migration_v13::apply(&mut transaction).await?;
            crate::migration_v14::apply(&mut transaction).await?;
            crate::migration_v15::apply(&mut transaction).await?;
            transaction.commit().await?;
            return Ok(());
        }

        for (table, column, statement) in V2_COLUMNS {
            add_column_if_missing(&mut transaction, table, column, statement).await?;
        }

        let legacy_rows = sqlx::query(
            "SELECT activity_events.project_identity, projects.id, projects.display_path
             FROM activity_events
             LEFT JOIN projects ON projects.identity = activity_events.project_identity
             GROUP BY activity_events.project_identity
             ORDER BY activity_events.project_identity, projects.id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        for row in legacy_rows {
            let identity: String = row.try_get("project_identity")?;
            if row.try_get::<Option<i64>, _>("id")?.is_none() {
                let stored_identity = if identity.trim().is_empty() {
                    LEGACY_BLANK_IDENTITY
                } else {
                    &identity
                };
                let id: i64 = sqlx::query_scalar("SELECT COALESCE(MAX(id), 0) + 1 FROM projects")
                    .fetch_one(&mut *transaction)
                    .await?;
                let display_path = if identity.trim().is_empty() {
                    "Legacy activity"
                } else {
                    ""
                };
                sqlx::query(
                    "INSERT INTO projects
                     (id, identity, display_path, name, normalized_name, created_at_us, updated_at_us)
                     VALUES (?, ?, ?, '', '', 0, 0)
                     ON CONFLICT(identity) DO NOTHING",
                )
                .bind(id)
                .bind(stored_identity)
                .bind(display_path)
                .execute(&mut *transaction)
                .await?;
            }
        }

        let projects = sqlx::query(
            "SELECT projects.id, projects.identity, projects.display_path
             FROM projects
             WHERE EXISTS (
                 SELECT 1 FROM activity_events
                 WHERE activity_events.project_identity = projects.identity
                    OR (
                        trim(activity_events.project_identity) = ''
                        AND projects.identity = '__legacy__'
                    )
             )
             ORDER BY projects.identity, projects.id",
        )
        .fetch_all(&mut *transaction)
        .await?;
        let mut used_names = ProjectNames::new();
        for row in projects {
            let id: i64 = row.try_get("id")?;
            let identity: String = row.try_get("identity")?;
            let display_path: String = row.try_get("display_path")?;
            let suggestion = suggested_name(&display_path, &identity);
            let name = used_names.allocate(&identity, &suggestion);
            sqlx::query("UPDATE projects SET name = ?, normalized_name = ? WHERE id = ?")
                .bind(name.display())
                .bind(name.normalized())
                .bind(id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query(
            "DELETE FROM projects
             WHERE NOT EXISTS (
                 SELECT 1 FROM activity_events
                 WHERE activity_events.project_identity = projects.identity
                    OR (
                        trim(activity_events.project_identity) = ''
                        AND projects.identity = '__legacy__'
                    )
             )",
        )
        .execute(&mut *transaction)
        .await?;

        rebuild_projects_with_name_check(&mut transaction).await?;

        sqlx::raw_sql(include_str!("../migrations/0002_project_context.sql"))
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "INSERT INTO activity_origins
             (identity, kind, resolution_source, display_path, routing_mode, default_project_id,
              setup_state, created_at_us, updated_at_us)
             SELECT projects.identity, 'unresolved', 'legacy_migrated', projects.display_path,
                    'dedicated', projects.id, 'unconfirmed', 0, 0
             FROM projects
             WHERE true
             ON CONFLICT(identity) DO NOTHING",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE activity_events
             SET origin_id = COALESCE(
                 (
                     SELECT id FROM activity_origins
                     WHERE identity = activity_events.project_identity
                 ),
                 (
                     SELECT id FROM activity_origins
                     WHERE trim(activity_events.project_identity) = ''
                       AND identity = '__legacy__'
                 )
             ),
             first_recorded_at_us = CASE
                 WHEN unixepoch(created_at) IS NULL THEN NULL
                 ELSE CAST(unixepoch(created_at) AS INTEGER) * 1000000
             END,
             first_recorded_at_provenance = CASE
                 WHEN unixepoch(created_at) IS NULL THEN NULL
                 ELSE 'legacy_recorded'
             END",
        )
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "WITH sequenced AS (
                 SELECT id, ROW_NUMBER() OVER (ORDER BY id) AS value FROM activity_events
             )
             UPDATE activity_events
             SET global_sequence = (
                 SELECT value FROM sequenced WHERE sequenced.id = activity_events.id
             )
             WHERE global_sequence IS NULL",
        )
        .execute(&mut *transaction)
        .await?;

        sqlx::query(
            "INSERT INTO ingest_dedupes (
                 provider, provider_session_id, provider_turn_id, activity_event_id
             )
             SELECT provider, provider_session_id, provider_turn_id, id
             FROM activity_events
             WHERE true
             ON CONFLICT(provider, provider_session_id, provider_turn_id) DO NOTHING",
        )
        .execute(&mut *transaction)
        .await?;

        let has_foreign_key_errors: bool =
            sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
                .fetch_optional(&mut *transaction)
                .await?
                .is_some();
        if has_foreign_key_errors {
            return Err(sqlx::Error::Protocol("foreign key check failed".to_owned()).into());
        }
        sqlx::query(
            "INSERT INTO schema_migrations (version, applied_at_us)
             VALUES (2, CAST(unixepoch('now') AS INTEGER) * 1000000)",
        )
        .execute(&mut *transaction)
        .await?;
        crate::migration_v3::apply(&mut transaction).await?;
        crate::migration_v4::apply(&mut transaction).await?;
        crate::migration_v5::apply(&mut transaction).await?;
        crate::migration_v6::apply(&mut transaction).await?;
        crate::migration_v7::apply(&mut transaction).await?;
        crate::migration_v8::apply(&mut transaction).await?;
        crate::migration_v9::apply(&mut transaction).await?;
        crate::migration_v10::apply(&mut transaction).await?;
        crate::migration_v11::apply(&mut transaction).await?;
        crate::migration_v12::apply(&mut transaction).await?;
        crate::migration_v13::apply(&mut transaction).await?;
        crate::migration_v14::apply(&mut transaction).await?;
        crate::migration_v15::apply(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }
}
