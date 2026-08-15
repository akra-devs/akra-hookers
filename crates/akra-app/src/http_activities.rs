use axum::{
    Json,
    extract::{Path, Query, State},
};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};

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
    include_subagent: Option<bool>,
    include_internal: Option<bool>,
    period: Option<String>,
}

#[derive(Default, Deserialize)]
pub(crate) struct ActivityDetailQuery {
    conversation_limit: Option<i64>,
    conversation_after_id: Option<i64>,
    include_subagent: Option<bool>,
    include_internal: Option<bool>,
}

pub(crate) async fn activities(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<akra_store::ActivitySummary>>, ApiError> {
    let scope = activity_scope(&query)?;
    let limit = page_limit(query.limit)?;
    let order = activity_order(query.order.as_deref())?;
    let activity_filter = activity_kind_filter(query.include_subagent, query.include_internal);
    let time_range = activity_time_range(query.period.as_deref())?;
    validate_cursor(query.after_id)?;
    state
        .store
        .activity_summaries_indexed_page_filtered_in_range(
            scope,
            query.after_id,
            limit,
            order,
            activity_filter,
            time_range,
        )
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
    let activity_filter = activity_kind_filter(query.include_subagent, query.include_internal);
    let time_range = activity_time_range(query.period.as_deref())?;
    state
        .store
        .activity_summary_count_filtered_in_range(scope, activity_filter, time_range)
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
    let activity_filter = activity_kind_filter(query.include_subagent, query.include_internal);
    validate_cursor(query.conversation_after_id)?;
    state
        .store
        .activity_detail_page_filtered(
            activity_id,
            query.conversation_after_id,
            limit,
            activity_filter,
        )
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

pub(crate) fn activity_kind_filter(
    include_subagent: Option<bool>,
    include_internal: Option<bool>,
) -> akra_store::ActivityKindFilter {
    akra_store::ActivityKindFilter::new(
        include_subagent.unwrap_or(true),
        include_internal.unwrap_or(true),
    )
}

pub(crate) fn activity_time_range(
    period: Option<&str>,
) -> Result<akra_store::ActivityTimeRange, ApiError> {
    let hours = match period.unwrap_or("all") {
        "all" => return Ok(akra_store::ActivityTimeRange::ALL),
        "day" => 24_i64,
        "week" => 24 * 7,
        "month" => 24 * 30,
        "quarter" => 24 * 90,
        _ => {
            return Err(ApiError::unprocessable(
                "invalid_period",
                "Activity period must be all, day, week, month, or quarter.",
            ));
        }
    };
    let now_us: i64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::unprocessable("invalid_period", "System time is unavailable."))?
        .as_micros()
        .try_into()
        .map_err(|_| ApiError::unprocessable("invalid_period", "System time is unavailable."))?;
    let duration_us = hours
        .checked_mul(60 * 60 * 1_000_000)
        .ok_or_else(|| ApiError::unprocessable("invalid_period", "Activity period overflowed."))?;
    Ok(akra_store::ActivityTimeRange::since(
        now_us.saturating_sub(duration_us),
    ))
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
