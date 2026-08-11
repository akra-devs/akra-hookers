use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};

use crate::{http::AppState, http_error::ApiError};

const DEFAULT_PAGE_SIZE: i64 = 100;
const MAX_PAGE_SIZE: i64 = 200;

#[derive(Deserialize)]
pub(crate) struct ActivityQuery {
    scope: Option<String>,
    project_id: Option<String>,
    project: Option<String>,
    limit: Option<i64>,
    after_id: Option<i64>,
    order: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct ActivityDetailQuery {
    conversation_limit: Option<i64>,
    conversation_after_id: Option<i64>,
}

pub(crate) async fn activities(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<akra_store::ActivitySummary>>, ApiError> {
    let scope = activity_scope(&query)?;
    let limit = page_limit(query.limit)?;
    let order = activity_order(query.order.as_deref())?;
    validate_cursor(query.after_id)?;
    state
        .store
        .activity_summaries_indexed_page(scope, query.after_id, limit, order)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

#[derive(Serialize)]
pub(crate) struct ActivityCountResponse {
    count: i64,
}

pub(crate) async fn activity_count(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<ActivityCountResponse>, ApiError> {
    let scope = activity_scope(&query)?;
    state
        .store
        .activity_summary_count(scope)
        .await
        .map(|count| Json(ActivityCountResponse { count }))
        .map_err(ApiError::from_store)
}

pub(crate) async fn activity_detail(
    State(state): State<AppState>,
    Path(activity_id): Path<i64>,
    Query(query): Query<ActivityDetailQuery>,
) -> Result<Json<akra_store::ActivityDetail>, ApiError> {
    let limit = page_limit(query.conversation_limit)?;
    validate_cursor(query.conversation_after_id)?;
    state
        .store
        .activity_detail_page(activity_id, query.conversation_after_id, limit)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

fn activity_scope(query: &ActivityQuery) -> Result<akra_store::ActivityScope, ApiError> {
    if query.project.is_some() {
        return Err(invalid_pagination(
            "Legacy project filters are not supported.",
        ));
    }
    match (query.scope.as_deref(), query.project_id.as_deref()) {
        (Some("all"), None) => Ok(akra_store::ActivityScope::All),
        (Some("inbox"), None) => Ok(akra_store::ActivityScope::Inbox),
        (Some("project"), Some(project_id)) => project_id
            .parse::<i64>()
            .ok()
            .filter(|project_id| *project_id > 0)
            .map(akra_store::ActivityScope::Project)
            .ok_or_else(|| invalid_pagination("A positive project_id is required.")),
        _ => Err(invalid_pagination("Activity scope is invalid.")),
    }
}

fn activity_order(order: Option<&str>) -> Result<akra_store::ActivityOrder, ApiError> {
    match order.unwrap_or("oldest") {
        "oldest" => Ok(akra_store::ActivityOrder::Oldest),
        "newest" => Ok(akra_store::ActivityOrder::Newest),
        _ => Err(invalid_pagination("Activity page order is invalid.")),
    }
}

fn page_limit(requested: Option<i64>) -> Result<i64, ApiError> {
    match requested.unwrap_or(DEFAULT_PAGE_SIZE) {
        limit @ 1..=MAX_PAGE_SIZE => Ok(limit),
        _ => Err(invalid_pagination("Page limit must be between 1 and 200.")),
    }
}

fn validate_cursor(cursor: Option<i64>) -> Result<(), ApiError> {
    if cursor.is_some_and(|cursor| cursor <= 0) {
        return Err(invalid_pagination("Page cursor must be a positive ID."));
    }
    Ok(())
}

fn invalid_pagination(message: &str) -> ApiError {
    ApiError::unprocessable("invalid_pagination", message)
}
