use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    http::AppState,
    http_activities::activity_time_range,
    http_error::ApiError,
    summarization::{CodexWorkCurator, SummarizationError},
};

const DEFAULT_LOG_LIMIT: i64 = 100;
const MAX_LOG_LIMIT: i64 = 200;

#[derive(Deserialize)]
pub(crate) struct CurationLogsQuery {
    project_id: i64,
    state: Option<String>,
    limit: Option<i64>,
    period: Option<String>,
    start_at_us: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct WorkItemsQuery {
    project_id: Option<i64>,
}

#[derive(Deserialize)]
pub(crate) struct ExcludeLogRequest {
    excluded: bool,
}

#[derive(Deserialize)]
pub(crate) struct WorkUpdateRequest {
    title: Option<String>,
    position_x: Option<f64>,
    position_y: Option<f64>,
}

#[derive(Deserialize)]
pub(crate) struct WorkEdgeRequest {
    source_work_item_id: i64,
    target_work_item_id: i64,
}

#[derive(Deserialize)]
pub(crate) struct ProposalRequest {
    project_id: i64,
    activity_ids: Vec<i64>,
}

#[derive(Deserialize)]
pub(crate) struct ApplyProposalRequest {
    groups: Vec<akra_store::CurationProposalGroup>,
}

#[derive(Serialize)]
pub(crate) struct WorkRevisionResponse {
    revision: i64,
}

pub(crate) async fn curation_logs(
    State(state): State<AppState>,
    Query(query): Query<CurationLogsQuery>,
) -> Result<Json<Vec<akra_store::CurationLogSummary>>, ApiError> {
    if query.project_id <= 0 {
        return Err(ApiError::unprocessable(
            "invalid_project_id",
            "A positive project_id is required.",
        ));
    }
    let filter = match query.state.as_deref().unwrap_or("unreviewed") {
        "unreviewed" => akra_store::CurationLogFilter::Unreviewed,
        "excluded" => akra_store::CurationLogFilter::Excluded,
        "organized" => akra_store::CurationLogFilter::Organized,
        "all" => akra_store::CurationLogFilter::All,
        _ => {
            return Err(ApiError::unprocessable(
                "invalid_curation_state",
                "Curation state must be unreviewed, excluded, organized, or all.",
            ));
        }
    };
    let limit = query.limit.unwrap_or(DEFAULT_LOG_LIMIT);
    if !(1..=MAX_LOG_LIMIT).contains(&limit) {
        return Err(ApiError::unprocessable(
            "invalid_page_limit",
            "Curation log limit must be between 1 and 200.",
        ));
    }
    state
        .store
        .curation_logs_in_range(
            query.project_id,
            filter,
            activity_time_range(query.period.as_deref(), query.start_at_us)?,
            limit,
        )
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn update_curation_log(
    State(state): State<AppState>,
    Path(activity_id): Path<i64>,
    Json(request): Json<ExcludeLogRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .set_activity_excluded(activity_id, request.excluded)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn delete_curation_log(
    State(state): State<AppState>,
    Path(activity_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .soft_delete_activity(activity_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn create_proposal(
    State(state): State<AppState>,
    Json(request): Json<ProposalRequest>,
) -> Result<Json<akra_store::CurationProposal>, ApiError> {
    let preparation = state
        .store
        .prepare_curation(request.project_id, &request.activity_ids)
        .await
        .map_err(ApiError::from_store)?;
    if let Some(cached) = preparation.cached() {
        return Ok(Json(cached.clone()));
    }
    let targets = state
        .codex
        .as_ref()
        .map(|control| Arc::clone(&control.targets))
        .ok_or_else(|| {
            ApiError::service_unavailable(
                "curation_runtime_unavailable",
                "A local Codex runtime is required to organize selected logs.",
            )
        })?;
    let curator = CodexWorkCurator::new(targets);
    let groups = curator
        .propose(preparation.input())
        .await
        .map_err(curation_error)?;
    state
        .store
        .save_curation_proposal(&preparation, groups)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn apply_proposal(
    State(state): State<AppState>,
    Path(proposal_id): Path<i64>,
    Json(request): Json<ApplyProposalRequest>,
) -> Result<Json<akra_store::CurationApplyResult>, ApiError> {
    state
        .store
        .apply_curation_proposal(proposal_id, request.groups)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn work_items(
    State(state): State<AppState>,
    Query(query): Query<WorkItemsQuery>,
) -> Result<Json<Vec<akra_store::WorkItemSummary>>, ApiError> {
    if query.project_id.is_some_and(|id| id <= 0) {
        return Err(ApiError::unprocessable(
            "invalid_project_id",
            "project_id must be positive.",
        ));
    }
    state
        .store
        .work_items(query.project_id)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn work_item(
    State(state): State<AppState>,
    Path(work_id): Path<i64>,
) -> Result<Json<akra_store::WorkItemDetail>, ApiError> {
    state
        .store
        .work_item(work_id)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn update_work_item(
    State(state): State<AppState>,
    Path(work_id): Path<i64>,
    Json(request): Json<WorkUpdateRequest>,
) -> Result<StatusCode, ApiError> {
    if request.title.is_none() && request.position_x.is_none() && request.position_y.is_none() {
        return Err(ApiError::unprocessable(
            "empty_work_update",
            "Provide a title or a complete position.",
        ));
    }
    if request.position_x.is_some() != request.position_y.is_some() {
        return Err(ApiError::unprocessable(
            "incomplete_work_position",
            "position_x and position_y must be provided together.",
        ));
    }
    if let Some(title) = request.title {
        state
            .store
            .rename_work(work_id, &title)
            .await
            .map_err(ApiError::from_store)?;
    }
    if let (Some(position_x), Some(position_y)) = (request.position_x, request.position_y) {
        state
            .store
            .update_work_position(work_id, position_x, position_y)
            .await
            .map_err(ApiError::from_store)?;
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_work_item(
    State(state): State<AppState>,
    Path(work_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_work(work_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn remove_work_log(
    State(state): State<AppState>,
    Path((work_id, activity_id)): Path<(i64, i64)>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .remove_work_log(work_id, activity_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn work_edges(
    State(state): State<AppState>,
    Query(query): Query<WorkItemsQuery>,
) -> Result<Json<Vec<akra_store::WorkEdgeSummary>>, ApiError> {
    state
        .store
        .work_edges(query.project_id)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

pub(crate) async fn create_work_edge(
    State(state): State<AppState>,
    Json(request): Json<WorkEdgeRequest>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .create_work_edge(request.source_work_item_id, request.target_work_item_id)
        .await
        .map(|()| StatusCode::CREATED)
        .map_err(ApiError::from_store)
}

pub(crate) async fn delete_work_edge(
    State(state): State<AppState>,
    Path(edge_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_work_edge(edge_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn work_revision(
    State(state): State<AppState>,
) -> Result<Json<WorkRevisionResponse>, ApiError> {
    state
        .store
        .work_revision()
        .await
        .map(|revision| Json(WorkRevisionResponse { revision }))
        .map_err(ApiError::from_store)
}

fn curation_error(error: SummarizationError) -> ApiError {
    match error {
        SummarizationError::CurationInputTooLarge(_) => ApiError::payload_too_large(
            "curation_input_too_large",
            "The selected summaries exceed the curation input budget.",
        ),
        SummarizationError::InvalidCurationOutput(_) | SummarizationError::Json(_) => {
            ApiError::service_unavailable(
                "curation_output_invalid",
                "Codex Spark returned an invalid grouping. Try again or select fewer logs.",
            )
        }
        _ => ApiError::service_unavailable(
            "curation_runtime_unavailable",
            "Codex Spark could not organize the selected logs. The logs were not changed.",
        ),
    }
}
