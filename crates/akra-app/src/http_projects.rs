use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{http::AppState, http_error::ApiError};

#[derive(Deserialize)]
pub(crate) struct ProjectNamePayload {
    name: String,
}

#[derive(Deserialize)]
pub(crate) struct ProjectMergePayload {
    target_project_id: i64,
}

pub(crate) async fn projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::ProjectSummary>>, ApiError> {
    state
        .store
        .projects()
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn create_project(
    State(state): State<AppState>,
    Json(payload): Json<ProjectNamePayload>,
) -> Result<(StatusCode, Json<akra_store::ProjectSummary>), ApiError> {
    state
        .store
        .create_project(&payload.name)
        .await
        .map(|project| (StatusCode::CREATED, Json(project)))
        .map_err(ApiError::from_store)
}

pub(crate) async fn rename_project(
    State(state): State<AppState>,
    Path(project_id): Path<i64>,
    Json(payload): Json<ProjectNamePayload>,
) -> Result<Json<akra_store::ProjectSummary>, ApiError> {
    state
        .store
        .rename_project(project_id, &payload.name)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn merge_project(
    State(state): State<AppState>,
    Path(source_project_id): Path<i64>,
    Json(payload): Json<ProjectMergePayload>,
) -> Result<Json<akra_store::ProjectSummary>, ApiError> {
    state
        .store
        .merge_projects(source_project_id, payload.target_project_id)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}
