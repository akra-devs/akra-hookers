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

pub fn app(token: &'static str, store: Arc<akra_store::ActivityStore>) -> Router {
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
        .with_state(store)
}

async fn ingest(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Json(payload): Json<IngressPayload>,
) -> Result<StatusCode, StatusCode> {
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
    State(store): State<Arc<akra_store::ActivityStore>>,
) -> Result<Json<Vec<akra_store::ProjectSummary>>, StatusCode> {
    store
        .projects()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn activities(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Query(query): Query<ActivityQuery>,
) -> Result<Json<Vec<akra_store::ActivitySummary>>, StatusCode> {
    store
        .activities_for_project(query.project.as_deref())
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn canvas(
    State(store): State<Arc<akra_store::ActivityStore>>,
) -> Result<Json<Vec<akra_store::CanvasNodeSummary>>, StatusCode> {
    store
        .canvas_nodes()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn create_canvas_edge(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Json(edge): Json<CanvasEdge>,
) -> Result<StatusCode, StatusCode> {
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
    State(store): State<Arc<akra_store::ActivityStore>>,
) -> Result<Json<Vec<akra_store::CanvasEdgeSummary>>, StatusCode> {
    store
        .canvas_edges()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn toggle_provider(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Path(provider): Path<String>,
    Json(toggle): Json<ProviderToggle>,
) -> Result<StatusCode, StatusCode> {
    if provider != "codex" {
        return Err(StatusCode::NOT_FOUND);
    }
    store
        .set_provider_enabled(&provider, toggle.enabled)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn provider(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Path(provider): Path<String>,
) -> Result<Json<akra_store::ProviderIntegration>, StatusCode> {
    if provider != "codex" {
        return Err(StatusCode::NOT_FOUND);
    }
    store
        .provider(&provider)
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn clear_canvas(
    State(store): State<Arc<akra_store::ActivityStore>>,
) -> Result<StatusCode, StatusCode> {
    store
        .clear_canvas()
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn delete_canvas_node(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Path(node_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    store
        .delete_canvas_node(node_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

async fn update_canvas_position(
    State(store): State<Arc<akra_store::ActivityStore>>,
    Path(node_id): Path<i64>,
    Json(position): Json<CanvasPosition>,
) -> Result<StatusCode, StatusCode> {
    store
        .update_canvas_position(node_id, position.position_x, position.position_y)
        .await
        .map(|_| StatusCode::NO_CONTENT)
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
