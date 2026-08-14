use std::sync::Arc;

use axum::{
    Json,
    body::{Body, Bytes},
    extract::State,
    http::{HeaderMap, Request, StatusCode, header},
    middleware::Next,
    response::Response,
};
use serde::Serialize;

use crate::{
    collector::{CollectorError, CollectorManager, ReceiveOutcome, RemoteCapture},
    http::AppState,
    http_error::ApiError,
    spool::SpoolError,
};

#[derive(Serialize)]
pub(crate) struct CaptureReceipt {
    capture_id: String,
    status: &'static str,
}

/// Authenticates the narrow collector ingress capability. It deliberately does
/// not share the dashboard token or browser CORS policy.
pub(crate) async fn authorize(
    request: Request<Body>,
    next: Next,
    collector: Arc<CollectorManager>,
) -> Result<Response, StatusCode> {
    let Some(presented) = bearer_token(request.headers()) else {
        return Err(StatusCode::UNAUTHORIZED);
    };
    if collector.authenticate(presented) {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

pub(crate) async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<(StatusCode, Json<CaptureReceipt>), ApiError> {
    if headers.get(header::CONTENT_ENCODING).is_some() {
        return Err(ApiError::unprocessable(
            "unsupported_content_encoding",
            "Collector captures must not use content encoding.",
        ));
    }
    let content_type = headers
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !content_type
        .split(';')
        .next()
        .is_some_and(|value| value.trim().eq_ignore_ascii_case("application/json"))
    {
        return Err(ApiError::unprocessable(
            "invalid_content_type",
            "Collector captures require application/json.",
        ));
    }
    let capture: RemoteCapture = serde_json::from_slice(&body).map_err(|_| {
        ApiError::unprocessable(
            "invalid_collector_capture",
            "Collector capture JSON is invalid.",
        )
    })?;
    let capture_id = capture.capture_id.clone();
    let collector = state
        .collector
        .ok_or_else(|| ApiError::not_found("Collector ingress is unavailable."))?;
    let outcome = tokio::task::spawn_blocking(move || collector.receive(capture))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(collector_error)?;
    let (status, receipt_status) = match outcome {
        ReceiveOutcome::Accepted => (StatusCode::ACCEPTED, "accepted"),
        ReceiveOutcome::Duplicate => (StatusCode::OK, "duplicate"),
    };
    Ok((
        status,
        Json(CaptureReceipt {
            capture_id,
            status: receipt_status,
        }),
    ))
}

pub(crate) async fn verify(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state
        .collector
        .ok_or_else(|| ApiError::not_found("Collector ingress is unavailable."))?;
    Ok(StatusCode::NO_CONTENT)
}

fn bearer_token(headers: &HeaderMap) -> Option<&str> {
    let mut values = headers.get_all(header::AUTHORIZATION).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        return None;
    }
    value.strip_prefix("Bearer ")
}

fn collector_error(error: CollectorError) -> ApiError {
    match error {
        CollectorError::Unauthorized => ApiError::unauthorized(),
        CollectorError::CaptureConflict => ApiError::conflict(
            "capture_id_conflict",
            "Capture ID was already used with different content.",
        ),
        CollectorError::InvalidRemoteCapture
        | CollectorError::Envelope(_)
        | CollectorError::Json(_) => {
            ApiError::unprocessable("invalid_collector_capture", "Collector capture is invalid.")
        }
        CollectorError::Spool(SpoolError::QueueFull { .. }) => ApiError::service_unavailable(
            "collector_queue_full",
            "Collector is temporarily at capacity. Retry later.",
        ),
        CollectorError::Spool(SpoolError::Oversized(_)) => ApiError::payload_too_large(
            "collector_capture_too_large",
            "Collector capture exceeds the accepted size.",
        ),
        CollectorError::Spool(_)
        | CollectorError::Io(_)
        | CollectorError::StatePoisoned
        | CollectorError::Clock
        | CollectorError::MissingParent => ApiError::internal(),
        CollectorError::InvalidEndpoint(_)
        | CollectorError::InsecureRemoteEndpoint
        | CollectorError::RemoteTokenRequired
        | CollectorError::InvalidToken
        | CollectorError::InvalidConfig
        | CollectorError::Http(_) => ApiError::unprocessable(
            "collector_unavailable",
            "Collector configuration is unavailable.",
        ),
    }
}
