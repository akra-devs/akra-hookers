use sqlx::{Sqlite, Transaction};

use crate::{
    ActivityStore, OriginSummary, StoreError,
    projects::{create_project_in, ensure_project_exists, rename_project_in},
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum OriginRoutingMode {
    Dedicated,
    Shared,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ProjectDestination {
    ProjectId(i64),
    NewProjectName(String),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OriginRoutingCommand {
    mode: OriginRoutingMode,
    destination: Option<ProjectDestination>,
    confirm: bool,
}

impl OriginRoutingCommand {
    pub fn dedicated(destination: ProjectDestination, confirm: bool) -> Self {
        Self {
            mode: OriginRoutingMode::Dedicated,
            destination: Some(destination),
            confirm,
        }
    }

    pub const fn shared(confirm: bool) -> Self {
        Self {
            mode: OriginRoutingMode::Shared,
            destination: None,
            confirm,
        }
    }
}

struct CurrentOrigin {
    mode: String,
    default_project_id: Option<i64>,
    setup_state: String,
}

impl ActivityStore {
    pub async fn configure_origin(
        &self,
        origin_id: i64,
        command: OriginRoutingCommand,
    ) -> Result<OriginSummary, StoreError> {
        if !command.confirm {
            return Err(StoreError::InvalidOriginTransition(
                "explicit confirmation is required".to_owned(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let current = load_origin(&mut transaction, origin_id).await?;
        match command.mode {
            OriginRoutingMode::Shared => {
                if command.destination.is_some() {
                    return Err(StoreError::InvalidOriginTransition(
                        "shared mode cannot have a destination".to_owned(),
                    ));
                }
                make_shared(&mut transaction, origin_id, &current).await?;
            }
            OriginRoutingMode::Dedicated => {
                let destination = command.destination.ok_or_else(|| {
                    StoreError::InvalidOriginTransition(
                        "dedicated mode requires a destination".to_owned(),
                    )
                })?;
                make_dedicated(&mut transaction, origin_id, &current, destination).await?;
            }
        }
        transaction.commit().await?;
        self.origin(origin_id).await
    }
}

async fn load_origin(
    transaction: &mut Transaction<'_, Sqlite>,
    origin_id: i64,
) -> Result<CurrentOrigin, StoreError> {
    let row = sqlx::query_as::<_, (String, Option<i64>, String)>(
        "SELECT routing_mode, default_project_id, setup_state
         FROM activity_origins WHERE id = ?",
    )
    .bind(origin_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::OriginNotFound(origin_id))?;
    Ok(CurrentOrigin {
        mode: row.0,
        default_project_id: row.1,
        setup_state: row.2,
    })
}

async fn make_shared(
    transaction: &mut Transaction<'_, Sqlite>,
    origin_id: i64,
    current: &CurrentOrigin,
) -> Result<(), StoreError> {
    if current.mode == "shared" {
        if current.setup_state != "confirmed" {
            confirm_origin(transaction, origin_id).await?;
        }
        return Ok(());
    }
    if current.mode != "dedicated" {
        return Err(StoreError::Invariant(format!(
            "unknown origin routing mode: {}",
            current.mode
        )));
    }
    let project_id = current.default_project_id.ok_or_else(|| {
        StoreError::Invariant("dedicated origin has no default project".to_owned())
    })?;
    sqlx::query(
        "INSERT INTO activity_project_assignments (
             activity_event_id, project_id, updated_at_us
         )
         SELECT id, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         FROM activity_events WHERE origin_id = ?
         ON CONFLICT(activity_event_id) DO UPDATE SET
             project_id = excluded.project_id,
             updated_at_us = excluded.updated_at_us",
    )
    .bind(project_id)
    .bind(origin_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE activity_origins SET
             routing_mode = 'shared', default_project_id = NULL, setup_state = 'confirmed',
             updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         WHERE id = ?",
    )
    .bind(origin_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn make_dedicated(
    transaction: &mut Transaction<'_, Sqlite>,
    origin_id: i64,
    current: &CurrentOrigin,
    destination: ProjectDestination,
) -> Result<(), StoreError> {
    let project_id = match destination {
        ProjectDestination::ProjectId(project_id) => {
            ensure_project_exists(transaction, project_id).await?;
            project_id
        }
        ProjectDestination::NewProjectName(name)
            if current.mode == "dedicated" && current.setup_state == "unconfirmed" =>
        {
            let project_id = current.default_project_id.ok_or_else(|| {
                StoreError::Invariant("dedicated origin has no default project".to_owned())
            })?;
            rename_project_in(transaction, project_id, &name).await?;
            project_id
        }
        ProjectDestination::NewProjectName(name) => create_project_in(transaction, &name).await?,
    };
    sqlx::query(
        "DELETE FROM activity_project_assignments
         WHERE activity_event_id IN (
             SELECT id FROM activity_events WHERE origin_id = ?
         )",
    )
    .bind(origin_id)
    .execute(&mut **transaction)
    .await?;
    if current.mode == "shared" {
        sqlx::query(
            "DELETE FROM conversation_routes
             WHERE EXISTS (
                 SELECT 1 FROM activity_events
                 WHERE activity_events.origin_id = ?
                   AND activity_events.provider = conversation_routes.provider
                   AND activity_events.provider_session_id =
                       conversation_routes.provider_session_id
             )
             AND NOT EXISTS (
                 SELECT 1
                 FROM activity_events AS other_events
                 JOIN activity_origins AS other_origins
                   ON other_origins.id = other_events.origin_id
                 WHERE other_events.origin_id != ?
                   AND other_origins.routing_mode = 'shared'
                   AND other_events.provider = conversation_routes.provider
                   AND other_events.provider_session_id =
                       conversation_routes.provider_session_id
             )",
        )
        .bind(origin_id)
        .bind(origin_id)
        .execute(&mut **transaction)
        .await?;
    }
    if current.mode != "dedicated"
        || current.default_project_id != Some(project_id)
        || current.setup_state != "confirmed"
    {
        sqlx::query(
            "UPDATE activity_origins SET
                 routing_mode = 'dedicated', default_project_id = ?, setup_state = 'confirmed',
                 updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             WHERE id = ?",
        )
        .bind(project_id)
        .bind(origin_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn confirm_origin(
    transaction: &mut Transaction<'_, Sqlite>,
    origin_id: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "UPDATE activity_origins SET
             setup_state = 'confirmed',
             updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         WHERE id = ?",
    )
    .bind(origin_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}
