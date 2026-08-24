use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{http::AppState, http_error::ApiError};

#[derive(Deserialize)]
pub(crate) struct CanvasPosition {
    position_x: f64,
    position_y: f64,
}

#[derive(Deserialize)]
pub(crate) struct CanvasEdge {
    source_node_id: i64,
    target_node_id: i64,
}

pub(crate) async fn canvas(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::CanvasNodeSummary>>, StatusCode> {
    state
        .store
        .canvas_nodes()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

#[derive(Serialize)]
pub(crate) struct CanvasRevision {
    revision: i64,
}

pub(crate) async fn canvas_revision(
    State(state): State<AppState>,
) -> Result<Json<CanvasRevision>, StatusCode> {
    state
        .store
        .canvas_revision()
        .await
        .map(|revision| Json(CanvasRevision { revision }))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn create_canvas_edge(
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

pub(crate) async fn canvas_edges(
    State(state): State<AppState>,
) -> Result<Json<Vec<akra_store::CanvasEdgeSummary>>, StatusCode> {
    state
        .store
        .canvas_edges()
        .await
        .map(Json)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn clear_canvas(State(state): State<AppState>) -> Result<StatusCode, StatusCode> {
    state
        .store
        .clear_canvas()
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn delete_canvas_edge(
    State(state): State<AppState>,
    Path(edge_id): Path<i64>,
) -> Result<StatusCode, StatusCode> {
    state
        .store
        .delete_canvas_edge(edge_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}

pub(crate) async fn delete_canvas_node(
    State(state): State<AppState>,
    Path(node_id): Path<i64>,
) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_canvas_node(node_id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn update_canvas_position(
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
