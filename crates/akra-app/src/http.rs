use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, Json, State},
    http::{Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post, put},
};
use serde::Deserialize;
use std::{
    net::IpAddr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
use tower_http::cors::{AllowOrigin, CorsLayer};

use crate::{
    http_activities::{activities, activity_count, activity_detail},
    http_assignments::assign_activities,
    http_canvas::{
        canvas, canvas_edges, canvas_revision, clear_canvas, create_canvas_edge,
        delete_canvas_edge, delete_canvas_node, update_canvas_position,
    },
    http_collector::{
        authorize as authorize_collector, ingest as collector_ingest, verify as collector_verify,
    },
    http_origins::{configure_origin, origins, project_origins},
    http_projects::{create_project, merge_project, projects, rename_project},
    http_providers::{
        configure_collector, provider, toggle_provider, toggle_provider_target, verify_collector,
    },
};

#[derive(Deserialize)]
struct IngressPayload {
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
}

#[derive(Clone)]
pub struct CodexLifecycleControl {
    pub(crate) targets: Arc<crate::codex_targets::CodexTargetRegistry>,
    pub(crate) capture_gate: crate::capture_gate::CaptureGate,
}

impl CodexLifecycleControl {
    pub fn new(
        targets: Arc<crate::codex_targets::CodexTargetRegistry>,
        capture_gate: crate::capture_gate::CaptureGate,
    ) -> Self {
        Self {
            targets,
            capture_gate,
        }
    }
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) store: Arc<akra_store::ActivityStore>,
    pub(crate) codex: Option<CodexLifecycleControl>,
    pub(crate) collector: Option<Arc<crate::collector::CollectorManager>>,
    pub(crate) provider_toggle_lock: Arc<Mutex<()>>,
}

pub fn app(token: &'static str, store: Arc<akra_store::ActivityStore>) -> Router {
    router(
        token,
        AppState {
            store,
            codex: None,
            collector: None,
            provider_toggle_lock: Arc::new(Mutex::new(())),
        },
    )
}

pub fn app_with_codex_lifecycle(
    token: &'static str,
    store: Arc<akra_store::ActivityStore>,
    lifecycle: Arc<akra_adapters::codex::CodexHookLifecycleSet>,
    command: String,
    capture_gate: crate::capture_gate::CaptureGate,
) -> Router {
    app_with_codex_targets(
        token,
        store,
        Arc::new(crate::codex_targets::CodexTargetRegistry::legacy(
            lifecycle, command,
        )),
        capture_gate,
    )
}

pub fn app_with_codex_targets(
    token: &'static str,
    store: Arc<akra_store::ActivityStore>,
    targets: Arc<crate::codex_targets::CodexTargetRegistry>,
    capture_gate: crate::capture_gate::CaptureGate,
) -> Router {
    app_with_codex_targets_and_collector(token, store, targets, capture_gate, None)
}

/// Builds an authenticated dashboard plus the narrow remote collector ingress
/// without attaching this host's Codex hook lifecycle. This is useful for a
/// collector-only runtime and for integration coverage of the token boundary.
pub fn app_with_collector(
    token: &'static str,
    store: Arc<akra_store::ActivityStore>,
    collector: Arc<crate::collector::CollectorManager>,
) -> Router {
    router(
        token,
        AppState {
            store,
            codex: None,
            collector: Some(collector),
            provider_toggle_lock: Arc::new(Mutex::new(())),
        },
    )
}

pub fn app_with_codex_targets_and_collector(
    token: &'static str,
    store: Arc<akra_store::ActivityStore>,
    targets: Arc<crate::codex_targets::CodexTargetRegistry>,
    capture_gate: crate::capture_gate::CaptureGate,
    collector: Option<Arc<crate::collector::CollectorManager>>,
) -> Router {
    router(
        token,
        AppState {
            store,
            codex: Some(CodexLifecycleControl::new(targets, capture_gate)),
            collector,
            provider_toggle_lock: Arc::new(Mutex::new(())),
        },
    )
}

fn router(token: &'static str, state: AppState) -> Router {
    let dashboard = Router::new()
        .route("/v1/ingest", post(ingest))
        .route("/v1/projects", get(projects).post(create_project))
        .route(
            "/v1/projects/{project_id}",
            axum::routing::patch(rename_project),
        )
        .route(
            "/v1/projects/{source_project_id}/merge",
            post(merge_project),
        )
        .route("/v1/origins", get(origins))
        .route("/v1/projects/{project_id}/origins", get(project_origins))
        .route(
            "/v1/origins/{origin_id}/routing",
            axum::routing::patch(configure_origin),
        )
        .route("/v1/activities", get(activities))
        .route("/v1/activities/count", get(activity_count))
        .route("/v1/activities/{activity_id}", get(activity_detail))
        .route("/v1/activity-assignments", post(assign_activities))
        .route("/v1/canvas", get(canvas).delete(clear_canvas))
        .route("/v1/canvas/revision", get(canvas_revision))
        .route(
            "/v1/canvas/edges",
            get(canvas_edges).post(create_canvas_edge),
        )
        .route("/v1/canvas/edges/{edge_id}", delete(delete_canvas_edge))
        .route(
            "/v1/providers/{provider}",
            get(provider).post(toggle_provider).patch(toggle_provider),
        )
        .route(
            "/v1/providers/{provider}/targets/{target_id}",
            axum::routing::patch(toggle_provider_target),
        )
        .route("/v1/providers/codex/collector", put(configure_collector))
        .route(
            "/v1/providers/codex/collector/verify",
            post(verify_collector),
        )
        .route(
            "/v1/canvas/{node_id}",
            delete(delete_canvas_node).patch(update_canvas_position),
        )
        .layer(middleware::from_fn(move |request, next| {
            authorize(request, next, token)
        }))
        .layer(
            CorsLayer::new()
                .allow_origin(AllowOrigin::predicate(|origin, _request_parts| {
                    origin.to_str().is_ok_and(is_local_dashboard_origin)
                }))
                .allow_methods([
                    Method::DELETE,
                    Method::GET,
                    Method::OPTIONS,
                    Method::PATCH,
                    Method::POST,
                    Method::PUT,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
        .with_state(state.clone());

    let Some(collector) = state.collector.clone() else {
        return dashboard;
    };
    let ingress = Router::new()
        .route("/v1/collector/ingest", post(collector_ingest))
        .route("/v1/collector/verify", get(collector_verify))
        .route_layer(middleware::from_fn(move |request, next| {
            authorize_collector(request, next, Arc::clone(&collector))
        }))
        .layer(DefaultBodyLimit::max(crate::spool::MAX_PENDING_ITEM_BYTES))
        .with_state(state);
    dashboard.merge(ingress)
}

fn is_local_dashboard_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    if url.scheme() != "http" || url.path() != "/" || url.query().is_some() {
        return false;
    }
    let Some(host) = url.host_str() else {
        return false;
    };
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<IpAddr>()
            .is_ok_and(|address| address.is_loopback())
}

async fn ingest(
    State(state): State<AppState>,
    Json(payload): Json<IngressPayload>,
) -> Result<StatusCode, StatusCode> {
    let _transition = state.provider_toggle_lock.lock().await;
    let store = &state.store;
    let enabled = match &state.codex {
        Some(control) => control
            .capture_gate
            .is_enabled()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
        None => store
            .provider_enabled("codex")
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    };
    if !enabled {
        return Ok(StatusCode::ACCEPTED);
    }
    let event = akra_core::ingress::IngressEvent::try_new(
        "codex",
        payload.session_id,
        payload.turn_id,
        payload.cwd,
        payload.prompt,
        None,
    )
    .map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
    let origin =
        akra_git::ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(event.cwd()))
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
            .origin;
    let captured_at_us = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_micros()).ok())
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;
    store
        .record(akra_store::RecordActivity::captured(
            event,
            origin,
            captured_at_us,
        ))
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::ACCEPTED)
}

async fn authorize(
    request: Request<Body>,
    next: Next,
    token: &'static str,
) -> Result<Response, StatusCode> {
    let expected = format!("Bearer {token}");
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|value| value == expected);
    if authorized {
        Ok(next.run(request).await)
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}
