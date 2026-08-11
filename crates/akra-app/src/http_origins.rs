use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;

use crate::{http::AppState, http_error::ApiError};

#[derive(Deserialize)]
pub(crate) struct OriginRoutingPayload {
    mode: String,
    destination: Option<DestinationPayload>,
    confirm: bool,
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

pub(crate) async fn origins(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::OriginSummary>>, ApiError> {
    state
        .store
        .origins()
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn project_origins(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
) -> Result<Json<Vec<akra_store::OriginSummary>>, ApiError> {
    state
        .store
        .origins_for_project(project_id)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn configure_origin(
    State(state): State<AppState>,
    Path(origin_id): Path<i64>,
    Json(payload): Json<OriginRoutingPayload>,
) -> Result<Json<akra_store::OriginSummary>, ApiError> {
    let command = routing_command(payload)?;
    state
        .store
        .configure_origin(origin_id, command)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

fn routing_command(
    payload: OriginRoutingPayload,
) -> Result<akra_store::OriginRoutingCommand, ApiError> {
    match (payload.mode.as_str(), payload.destination) {
        ("shared", None) => Ok(akra_store::OriginRoutingCommand::shared(payload.confirm)),
        ("shared", Some(_)) => Err(ApiError::unprocessable(
            "invalid_origin_routing",
            "Shared routing cannot include a project destination.",
        )),
        ("dedicated", Some(destination)) => Ok(akra_store::OriginRoutingCommand::dedicated(
            match destination {
                DestinationPayload::Existing(destination) => {
                    akra_store::ProjectDestination::ProjectId(destination.project_id)
                }
                DestinationPayload::New(destination) => {
                    akra_store::ProjectDestination::NewProjectName(destination.new_project_name)
                }
            },
            payload.confirm,
        )),
        ("dedicated", None) | (_, _) => Err(ApiError::unprocessable(
            "invalid_origin_routing",
            "Origin routing mode and destination do not match.",
        )),
    }
}
