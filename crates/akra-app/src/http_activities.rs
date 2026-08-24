use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
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
    include_internal: Option<bool>,
    period: Option<String>,
    start_at_us: Option<i64>,
}

#[derive(Default, Deserialize)]
pub(crate) struct ActivityDetailQuery {
    conversation_limit: Option<i64>,
    conversation_after_id: Option<i64>,
    conversation_offset: Option<i64>,
    include_internal: Option<bool>,
}

pub(crate) async fn activities(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<akra_store::ActivitySummary>>, ApiError> {
    let scope = activity_scope(&query)?;
    let limit = page_limit(query.limit)?;
    let order = activity_order(query.order.as_deref())?;
    let activity_filter = activity_kind_filter(query.include_internal);
    let time_range = activity_time_range(query.period.as_deref(), query.start_at_us)?;
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
    let activity_filter = activity_kind_filter(query.include_internal);
    let time_range = activity_time_range(query.period.as_deref(), query.start_at_us)?;
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
    let activity_filter = activity_kind_filter(query.include_internal);
    validate_cursor(query.conversation_after_id)?;
    let detail = match query.conversation_offset {
        Some(offset) => {
            if query.conversation_after_id.is_some() || offset < 0 {
                return Err(invalid_pagination(
                    "Conversation offset must be non-negative and cannot be combined with a cursor.",
                ));
            }
            state
                .store
                .activity_detail_offset_page_filtered(activity_id, offset, limit, activity_filter)
                .await
        }
        None => {
            state
                .store
                .activity_detail_page_filtered(
                    activity_id,
                    query.conversation_after_id,
                    limit,
                    activity_filter,
                )
                .await
        }
    };
    detail.map(Json).map_err(ApiError::from_store)
}

pub(crate) async fn delete_activity(
    State(state): State<AppState>,
    Path(activity_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_activity(activity_id)
        .await
        .map(|()| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn regenerate_result_summary(
    State(state): State<AppState>,
    Path(activity_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    match state
        .store
        .regenerate_result_summary(activity_id, now_us()?)
        .await
        .map_err(ApiError::from_store)?
    {
        akra_store::ResultSummaryRegenerationOutcome::Scheduled
        | akra_store::ResultSummaryRegenerationOutcome::AlreadyPending => Ok(StatusCode::ACCEPTED),
        akra_store::ResultSummaryRegenerationOutcome::Unavailable => Err(ApiError::unprocessable(
            "result_summary_regeneration_unavailable",
            "The original assistant result is no longer available for regeneration.",
        )),
    }
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
    include_internal: Option<bool>,
) -> akra_store::ActivityKindFilter {
    akra_store::ActivityKindFilter::new(false, include_internal.unwrap_or(true))
}

pub(crate) fn activity_time_range(
    period: Option<&str>,
    start_at_us: Option<i64>,
) -> Result<akra_store::ActivityTimeRange, ApiError> {
    let now_us = now_us()?;
    let hours = match period.unwrap_or("all") {
        "all" if start_at_us.is_none() => return Ok(akra_store::ActivityTimeRange::ALL),
        "today" => {
            let start_at_us = start_at_us.ok_or_else(|| {
                ApiError::unprocessable(
                    "invalid_period",
                    "Today requires the browser's local calendar-day start.",
                )
            })?;
            let maximum_calendar_day_us = 26_i64 * 60 * 60 * 1_000_000;
            if start_at_us <= 0
                || start_at_us > now_us
                || start_at_us < now_us.saturating_sub(maximum_calendar_day_us)
            {
                return Err(ApiError::unprocessable(
                    "invalid_period",
                    "Today start must be a recent local calendar-day boundary.",
                ));
            }
            return Ok(akra_store::ActivityTimeRange::since(start_at_us));
        }
        "day" => 24_i64,
        "week" => 24 * 7,
        "month" => 24 * 30,
        "quarter" => 24 * 90,
        _ => {
            return Err(ApiError::unprocessable(
                "invalid_period",
                "Activity period must be all, today, day, week, month, or quarter.",
            ));
        }
    };
    if start_at_us.is_some() {
        return Err(ApiError::unprocessable(
            "invalid_period",
            "A calendar-day start is only valid for the today period.",
        ));
    }
    let duration_us = hours
        .checked_mul(60 * 60 * 1_000_000)
        .ok_or_else(|| ApiError::unprocessable("invalid_period", "Activity period overflowed."))?;
    Ok(akra_store::ActivityTimeRange::since(
        now_us.saturating_sub(duration_us),
    ))
}

fn now_us() -> Result<i64, ApiError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| {
            ApiError::unprocessable("system_time_unavailable", "System time is unavailable.")
        })?
        .as_micros()
        .try_into()
        .map_err(|_| {
            ApiError::unprocessable("system_time_unavailable", "System time is unavailable.")
        })
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
