use axum::{
    Router,
    body::Body,
    extract::{Json, Path, Query, State},
    http::{Method, Request, StatusCode, header},
    middleware::{self, Next},
    response::Response,
    routing::{delete, get, post},
};
use serde::Deserialize;
use std::sync::Arc;
use tower_http::cors::{AllowOrigin, CorsLayer};

#[derive(Deserialize)]
struct IngressPayload {
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
}

#[derive(Deserialize)]
struct CanvasPosition {
    position_x: f64,
    position_y: f64,
}

#[derive(Deserialize)]
struct CanvasEdge {
    source_node_id: i64,
    target_node_id: i64,
}

#[derive(Deserialize)]
struct ProviderToggle {
    enabled: bool,
}

#[derive(Deserialize)]
struct ActivityQuery {
    project: Option<String>,
}

#[derive(Clone)]
pub struct CodexLifecycleControl {
    lifecycle: Arc<akra_adapters::codex::CodexHookLifecycle>,
    command: Arc<str>,
}

impl CodexLifecycleControl {
    pub fn new(lifecycle: Arc<akra_adapters::codex::CodexHookLifecycle>, command: String) -> Self {
        Self {
            lifecycle,
            command: Arc::from(command),
        }
    }
}

#[derive(Clone)]
struct AppState {
    store: Arc<akra_store::ActivityStore>,
    codex: Option<CodexLifecycleControl>,
}

pub fn app(token: &'static str, store: Arc<akra_store::ActivityStore>) -> Router {
    router(token, AppState { store, codex: None })
}

pub fn app_with_codex_lifecycle(
    token: &'static str,
    store: Arc<akra_store::ActivityStore>,
    lifecycle: Arc<akra_adapters::codex::CodexHookLifecycle>,
    command: String,
) -> Router {
    router(
        token,
        AppState {
            store,
            codex: Some(CodexLifecycleControl::new(lifecycle, command)),
        },
    )
}

fn router(token: &'static str, state: AppState) -> Router {
    Router::new()
        .route("/v1/ingest", post(ingest))
        .route("/v1/projects", get(projects))
        .route("/v1/activities", get(activities))
        .route("/v1/canvas", get(canvas).delete(clear_canvas))
        .route(
            "/v1/canvas/edges",
            get(canvas_edges).post(create_canvas_edge),
        )
        .route(
            "/v1/providers/{provider}",
            get(provider).post(toggle_provider).patch(toggle_provider),
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
                    origin.to_str().is_ok_and(|origin| {
                        origin.starts_with("http://127.0.0.1:")
                            || origin.starts_with("http://localhost:")
                    })
                }))
                .allow_methods([
                    Method::DELETE,
                    Method::GET,
                    Method::OPTIONS,
                    Method::PATCH,
                    Method::POST,
                ])
                .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]),
        )
        .with_state(state)
}

async fn ingest(
    State(state): State<AppState>,
    Json(payload): Json<IngressPayload>,
) -> Result<StatusCode, StatusCode> {
    let store = &state.store;
    if !store
        .provider_enabled("codex")
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Ok(StatusCode::ACCEPTED);
    }
    store
        .record(
            "codex",
            &payload.session_id,
            &payload.turn_id,
            &payload.cwd,
            &payload.prompt,
        )
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(StatusCode::ACCEPTED)
}

async fn projects(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::ProjectSummary>>, StatusCode> {
    let store = &state.store;
    store
        .projects()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn activities(
    State(state): State<AppState>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<akra_store::ActivitySummary>>, StatusCode> {
    let store = &state.store;
    store
        .activities_for_project(query.project.as_deref())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn canvas(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::CanvasNodeSummary>>, StatusCode> {
    let store = &state.store;
    store
        .canvas_nodes()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_canvas_edge(
    State(state): State<AppState>,
    Json(edge): Json<CanvasEdge>,
) -> Result<StatusCode, StatusCode> {
    let store = &state.store;
    if edge.source_node_id == edge.target_node_id {
        return Err(StatusCode::UNPROCESSABLE_ENTITY);
    }
    if !store
        .canvas_node_exists(edge.source_node_id)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        || !store
            .canvas_node_exists(edge.target_node_id)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    {
        return Err(StatusCode::NOT_FOUND);
    }
    store
        .create_canvas_edge(edge.source_node_id, edge.target_node_id)
        .await
        .map(|_| StatusCode::CREATED)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn canvas_edges(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::CanvasEdgeSummary>>, StatusCode> {
    let store = &state.store;
    store
        .canvas_edges()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn toggle_provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(toggle): Json<ProviderToggle>,
) -> Result<StatusCode, StatusCode> {
    if provider != "codex" {
        return Err(StatusCode::NOT_FOUND);
    }
    if let Some(codex) = state.codex {
        update_global_codex_hook(codex, toggle.enabled).await?;
    }
    state
        .store
        .set_provider_enabled(&provider, toggle.enabled)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<akra_store::ProviderIntegration>, StatusCode> {
    if provider != "codex" {
        return Err(StatusCode::NOT_FOUND);
    }
    if let Some(codex) = state.codex {
        let enabled = global_codex_hook_enabled(codex).await?;
        return Ok(Json(akra_store::ProviderIntegration { provider, enabled }));
    }
    state
        .store
        .provider(&provider)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn clear_canvas(State(state): State<AppState>) -> Result<StatusCode, StatusCode> {
    state
        .store
        .clear_canvas()
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn delete_canvas_node(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .store
        .delete_canvas_node(node_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn update_canvas_position(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
    Json(position): Json<CanvasPosition>,
) -> Result<StatusCode, StatusCode> {
    state
        .store
        .update_canvas_position(node_id, position.position_x, position.position_y)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn update_global_codex_hook(
    control: CodexLifecycleControl,
    enabled: bool,
) -> Result<(), StatusCode> {
    let lifecycle = Arc::clone(&control.lifecycle);
    let command = Arc::clone(&control.command);
    tokio::task::spawn_blocking(move || {
        if enabled {
            lifecycle.enable(&command)
        } else {
            lifecycle.disable()
        }
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn global_codex_hook_enabled(control: CodexLifecycleControl) -> Result<bool, StatusCode> {
    let lifecycle = Arc::clone(&control.lifecycle);
    tokio::task::spawn_blocking(move || lifecycle.is_enabled())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
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
