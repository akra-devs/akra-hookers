use std::collections::BTreeSet;

use serde::Serialize;
use sqlx::{Sqlite, Transaction};

use crate::{
    ActivityStore, StoreError,
    projects::{create_project_in, ensure_project_exists},
};

pub const MAX_ACTIVITY_ASSIGNMENT_BATCH: usize = 100;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AssignmentDestination {
    ProjectId(i64),
    NewProjectName(String),
    Inbox,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FutureRouteAction {
    Unchanged,
    Set,
    Clear,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ActivityAssignmentCommand {
    activity_ids: Vec<i64>,
    destination: AssignmentDestination,
    future_route: FutureRouteAction,
}

impl ActivityAssignmentCommand {
    pub fn new(
        activity_ids: Vec<i64>,
        destination: AssignmentDestination,
        future_route: FutureRouteAction,
    ) -> Self {
        Self {
            activity_ids,
            destination,
            future_route,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct ActivityAssignmentResult {
    pub activity_ids: Vec<i64>,
    pub project_id: Option<i64>,
    pub future_route: String,
}

struct SelectedActivity {
    provider: String,
    session_id: String,
}

impl ActivityStore {
    pub async fn assign_activities(
        &self,
        command: ActivityAssignmentCommand,
    ) -> Result<ActivityAssignmentResult, StoreError> {
        if command.activity_ids.len() > MAX_ACTIVITY_ASSIGNMENT_BATCH {
            return Err(invalid("at most 100 activities can be assigned at once"));
        }
        let activity_ids = command
            .activity_ids
            .into_iter()
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect::<Vec<_>>();
        if activity_ids.is_empty() {
            return Err(invalid("at least one activity is required"));
        }
        if command.future_route == FutureRouteAction::Set
            && command.destination == AssignmentDestination::Inbox
        {
            return Err(invalid("a future route requires a project destination"));
        }
        let mut transaction = self.pool.begin().await?;
        let selected = load_selected(&mut transaction, &activity_ids).await?;
        let route_key = route_key(&selected, command.future_route)?;
        let project_id = match command.destination {
            AssignmentDestination::ProjectId(project_id) => {
                ensure_project_exists(&mut transaction, project_id).await?;
                Some(project_id)
            }
            AssignmentDestination::NewProjectName(name) => {
                Some(create_project_in(&mut transaction, &name).await?)
            }
            AssignmentDestination::Inbox => None,
        };
        for activity_id in &activity_ids {
            assign_one(&mut transaction, *activity_id, project_id).await?;
        }
        apply_future_route(
            &mut transaction,
            route_key.as_ref(),
            project_id,
            command.future_route,
        )
        .await?;
        transaction.commit().await?;
        Ok(ActivityAssignmentResult {
            activity_ids,
            project_id,
            future_route: route_label(command.future_route).to_owned(),
        })
    }
}

async fn load_selected(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_ids: &[i64],
) -> Result<Vec<SelectedActivity>, StoreError> {
    let mut selected = Vec::with_capacity(activity_ids.len());
    for activity_id in activity_ids {
        let row = sqlx::query_as::<_, (String, String, Option<String>)>(
            "SELECT activity_events.provider, activity_events.provider_session_id,
                    activity_origins.routing_mode
              FROM activity_events
              LEFT JOIN activity_origins ON activity_origins.id = activity_events.origin_id
              WHERE activity_events.id = ?
                AND activity_events.deleted_at_us IS NULL",
        )
        .bind(activity_id)
        .fetch_optional(&mut **transaction)
        .await?
        .ok_or(StoreError::ActivityNotFound(*activity_id))?;
        if row.2.as_deref() != Some("shared") {
            return Err(invalid("every selected activity must have a shared origin"));
        }
        selected.push(SelectedActivity {
            provider: row.0,
            session_id: row.1,
        });
    }
    Ok(selected)
}

fn route_key(
    selected: &[SelectedActivity],
    action: FutureRouteAction,
) -> Result<Option<(String, String)>, StoreError> {
    if action == FutureRouteAction::Unchanged {
        return Ok(None);
    }
    let first = selected
        .first()
        .ok_or_else(|| invalid("at least one activity is required"))?;
    if selected.iter().any(|activity| {
        activity.provider != first.provider || activity.session_id != first.session_id
    }) {
        return Err(invalid("future routes require one provider conversation"));
    }
    Ok(Some((first.provider.clone(), first.session_id.clone())))
}

async fn assign_one(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_id: i64,
    project_id: Option<i64>,
) -> Result<(), StoreError> {
    if let Some(project_id) = project_id {
        sqlx::query(
            "INSERT INTO activity_project_assignments (
                 activity_event_id, project_id, updated_at_us
             ) VALUES (
                 ?, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             )
             ON CONFLICT(activity_event_id) DO UPDATE SET
                 project_id = excluded.project_id,
                 updated_at_us = excluded.updated_at_us
             WHERE activity_project_assignments.project_id != excluded.project_id",
        )
        .bind(activity_id)
        .bind(project_id)
        .execute(&mut **transaction)
        .await?;
    } else {
        sqlx::query("DELETE FROM activity_project_assignments WHERE activity_event_id = ?")
            .bind(activity_id)
            .execute(&mut **transaction)
            .await?;
    }
    Ok(())
}

async fn apply_future_route(
    transaction: &mut Transaction<'_, Sqlite>,
    route_key: Option<&(String, String)>,
    project_id: Option<i64>,
    action: FutureRouteAction,
) -> Result<(), StoreError> {
    let Some((provider, session_id)) = route_key else {
        return Ok(());
    };
    match action {
        FutureRouteAction::Unchanged => {}
        FutureRouteAction::Set => {
            let project_id = project_id.ok_or_else(|| invalid("route destination is missing"))?;
            sqlx::query(
                "INSERT INTO conversation_routes (
                     provider, provider_session_id, project_id, updated_at_us
                 ) VALUES (
                     ?, ?, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
                 )
                 ON CONFLICT(provider, provider_session_id) DO UPDATE SET
                     project_id = excluded.project_id,
                     updated_at_us = excluded.updated_at_us
                 WHERE conversation_routes.project_id != excluded.project_id",
            )
            .bind(provider)
            .bind(session_id)
            .bind(project_id)
            .execute(&mut **transaction)
            .await?;
        }
        FutureRouteAction::Clear => {
            sqlx::query(
                "DELETE FROM conversation_routes
                 WHERE provider = ? AND provider_session_id = ?",
            )
            .bind(provider)
            .bind(session_id)
            .execute(&mut **transaction)
            .await?;
        }
    }
    Ok(())
}

const fn route_label(action: FutureRouteAction) -> &'static str {
    match action {
        FutureRouteAction::Unchanged => "unchanged",
        FutureRouteAction::Set => "set",
        FutureRouteAction::Clear => "clear",
    }
}

fn invalid(message: &str) -> StoreError {
    StoreError::InvalidActivityAssignment(message.to_owned())
}
