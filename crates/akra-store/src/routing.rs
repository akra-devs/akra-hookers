use std::path::Path;

use akra_git::{ProjectOriginKind, ProjectOriginSnapshot};
use sqlx::{Row, SqliteConnection};

use crate::{
    ActivityStore, ProjectName, ProjectNames, RecordActivity, StoreError,
    projects::allocate_project_id,
};

pub(crate) struct OriginRouting {
    pub(crate) id: i64,
    mode: String,
    default_project_id: Option<i64>,
}

pub(crate) async fn ensure_origin(
    connection: &mut SqliteConnection,
    command: &RecordActivity,
) -> Result<OriginRouting, StoreError> {
    let origin = command.origin();
    if let Some((id, mode, default_project_id)) = sqlx::query_as::<_, (i64, String, Option<i64>)>(
        "SELECT id, routing_mode, default_project_id
             FROM activity_origins WHERE identity = ?",
    )
    .bind(&origin.identity)
    .fetch_optional(&mut *connection)
    .await?
    {
        return Ok(OriginRouting {
            id,
            mode,
            default_project_id,
        });
    }

    let project_id = ensure_project(connection, origin).await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO activity_origins (
             identity, kind, resolution_source, display_path, routing_mode,
             default_project_id, setup_state, created_at_us, updated_at_us
         ) VALUES (
             ?, ?, ?, ?, 'dedicated', ?, 'unconfirmed',
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER),
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         )
         RETURNING id",
    )
    .bind(&origin.identity)
    .bind(origin_kind(&origin.kind))
    .bind(command.resolution_source())
    .bind(origin.display_path.to_string_lossy().as_ref())
    .bind(project_id)
    .fetch_one(&mut *connection)
    .await?;
    Ok(OriginRouting {
        id,
        mode: "dedicated".to_owned(),
        default_project_id: Some(project_id),
    })
}

pub(crate) async fn assignment_for(
    connection: &mut SqliteConnection,
    origin: &OriginRouting,
    provider: &str,
    provider_session_id: &str,
) -> Result<Option<i64>, StoreError> {
    match origin.mode.as_str() {
        "dedicated" => {
            if origin.default_project_id.is_none() {
                return Err(StoreError::Invariant(
                    "dedicated origin has no project".to_owned(),
                ));
            }
            Ok(None)
        }
        "shared" => Ok(sqlx::query_scalar(
            "SELECT project_id FROM conversation_routes
             WHERE provider = ? AND provider_session_id = ?",
        )
        .bind(provider)
        .bind(provider_session_id)
        .fetch_optional(&mut *connection)
        .await?),
        mode => Err(StoreError::Invariant(format!(
            "unknown origin routing mode: {mode}"
        ))),
    }
}

impl ActivityStore {
    pub async fn remember_conversation_route(
        &self,
        provider: &str,
        provider_session_id: &str,
        project_id: i64,
    ) -> Result<bool, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let existing: Option<i64> = sqlx::query_scalar(
            "SELECT project_id FROM conversation_routes
             WHERE provider = ? AND provider_session_id = ?",
        )
        .bind(provider)
        .bind(provider_session_id)
        .fetch_optional(&mut *transaction)
        .await?;
        if existing == Some(project_id) {
            transaction.commit().await?;
            return Ok(false);
        }
        sqlx::query(
            "INSERT INTO conversation_routes (
                 provider, provider_session_id, project_id, updated_at_us
             ) VALUES (
                 ?, ?, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             )
             ON CONFLICT(provider, provider_session_id) DO UPDATE SET
                 project_id = excluded.project_id,
                 updated_at_us = excluded.updated_at_us",
        )
        .bind(provider)
        .bind(provider_session_id)
        .bind(project_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(true)
    }
}

async fn ensure_project(
    connection: &mut SqliteConnection,
    origin: &ProjectOriginSnapshot,
) -> Result<i64, StoreError> {
    if let Some(id) = sqlx::query_scalar::<_, i64>("SELECT id FROM projects WHERE identity = ?")
        .bind(&origin.identity)
        .fetch_optional(&mut *connection)
        .await?
    {
        return Ok(id);
    }
    let existing = sqlx::query("SELECT id, name FROM projects ORDER BY id")
        .fetch_all(&mut *connection)
        .await?;
    let mut names = ProjectNames::new();
    for row in existing {
        let id: i64 = row.try_get("id")?;
        let name: String = row.try_get("name")?;
        names.allocate(&format!("project:{id}"), &name);
    }
    let suggestion = ProjectName::suggest_from_path(Path::new(&origin.display_path));
    let name = names.allocate(&origin.identity, suggestion.display());
    let id = allocate_project_id(connection).await?;
    sqlx::query(
        "INSERT INTO projects (
             id, identity, display_path, name, normalized_name, created_at_us, updated_at_us
         ) VALUES (
             ?, ?, ?, ?, ?,
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER),
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         )
         ",
    )
    .bind(id)
    .bind(&origin.identity)
    .bind(origin.display_path.to_string_lossy().as_ref())
    .bind(name.display())
    .bind(name.normalized())
    .execute(&mut *connection)
    .await?;
    Ok(id)
}

const fn origin_kind(kind: &ProjectOriginKind) -> &'static str {
    match kind {
        ProjectOriginKind::Git => "git",
        ProjectOriginKind::Directory => "directory",
        ProjectOriginKind::Unresolved => "unresolved",
    }
}
