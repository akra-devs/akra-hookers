use axum::{Json, extract::State};
use serde::{Deserialize, Deserializer};

use crate::{http::AppState, http_error::ApiError};

#[derive(Deserialize)]
pub(crate) struct ActivityAssignmentPayload {
    activity_ids: Vec<i64>,
    #[serde(default)]
    destination: RequiredDestination,
    #[serde(default)]
    future_route: FutureRoutePayload,
}

#[derive(Default)]
enum RequiredDestination {
    #[default]
    Missing,
    Present(Option<DestinationPayload>),
}

impl<'de> Deserialize<'de> for RequiredDestination {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Option::<DestinationPayload>::deserialize(deserializer).map(Self::Present)
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum DestinationPayload {
    Existing(ExistingDestination),
    New(NewDestination),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExistingDestination {
    project_id: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewDestination {
    new_project_name: String,
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum FutureRoutePayload {
    #[default]
    Unchanged,
    Set,
    Clear,
}

pub(crate) async fn assign_activities(
    State(state): State<AppState>,
    Json(payload): Json<ActivityAssignmentPayload>,
) -> Result<Json<akra_store::ActivityAssignmentResult>, ApiError> {
    let command = assignment_command(payload)?;
    state
        .store
        .assign_activities(command)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

fn assignment_command(
    payload: ActivityAssignmentPayload,
) -> Result<akra_store::ActivityAssignmentCommand, ApiError> {
    let destination = match payload.destination {
        RequiredDestination::Missing => {
            return Err(ApiError::unprocessable(
                "missing_assignment_destination",
                "Activity assignment requires a destination.",
            ));
        }
        RequiredDestination::Present(None) => akra_store::AssignmentDestination::Inbox,
        RequiredDestination::Present(Some(DestinationPayload::Existing(destination))) => {
            akra_store::AssignmentDestination::ProjectId(destination.project_id)
        }
        RequiredDestination::Present(Some(DestinationPayload::New(destination))) => {
            akra_store::AssignmentDestination::NewProjectName(destination.new_project_name)
        }
    };
    let future_route = match payload.future_route {
        FutureRoutePayload::Unchanged => akra_store::FutureRouteAction::Unchanged,
        FutureRoutePayload::Set => akra_store::FutureRouteAction::Set,
        FutureRoutePayload::Clear => akra_store::FutureRouteAction::Clear,
    };
    Ok(akra_store::ActivityAssignmentCommand::new(
        payload.activity_ids,
        destination,
        future_route,
    ))
}
