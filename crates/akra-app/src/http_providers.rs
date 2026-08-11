use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::Deserialize;

use crate::{
    capture_gate::{disable_codex_capture, enable_codex_capture},
    http::{AppState, CodexLifecycleControl},
    http_error::ApiError,
};

#[derive(Deserialize)]
pub(crate) struct ProviderToggle {
    enabled: bool,
}

pub(crate) async fn toggle_provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(toggle): Json<ProviderToggle>,
) -> Result<StatusCode, ApiError> {
    if provider != "codex" {
        return Err(ApiError::not_found("Provider not found."));
    }
    let _transition = state.provider_toggle_lock.lock().await;
    if let Some(codex) = state.codex.clone() {
        let previous_enabled = global_codex_capture_enabled(codex.clone()).await?;
        update_global_codex_capture(codex.clone(), toggle.enabled).await?;
        if state
            .store
            .set_provider_enabled(&provider, toggle.enabled)
            .await
            .is_err()
        {
            let _ = update_global_codex_capture(codex, previous_enabled).await;
            return Err(ApiError::internal());
        }
        return Ok(StatusCode::NO_CONTENT);
    }
    state
        .store
        .set_provider_enabled(&provider, toggle.enabled)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<akra_store::ProviderIntegration>, ApiError> {
    if provider != "codex" {
        return Err(ApiError::not_found("Provider not found."));
    }
    let _transition = state.provider_toggle_lock.lock().await;
    if let Some(codex) = state.codex {
        let enabled = global_codex_capture_enabled(codex).await?;
        return Ok(Json(akra_store::ProviderIntegration { provider, enabled }));
    }
    state
        .store
        .provider(&provider)
        .await
        .map(Json)
        .map_err(ApiError::from_store)
}

async fn update_global_codex_capture(
    control: CodexLifecycleControl,
    enabled: bool,
) -> Result<(), ApiError> {
    let lifecycle = Arc::clone(&control.lifecycle);
    let command = Arc::clone(&control.command);
    let capture_gate = control.capture_gate;
    tokio::task::spawn_blocking(move || {
        if enabled {
            enable_codex_capture(&capture_gate, &lifecycle, &command)
                .map_err(|error| error.to_string())?;
        } else {
            disable_codex_capture(&capture_gate, &lifecycle).map_err(|error| error.to_string())?;
        }
        Ok::<(), String>(())
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_| ApiError::internal())
}

async fn global_codex_capture_enabled(control: CodexLifecycleControl) -> Result<bool, ApiError> {
    tokio::task::spawn_blocking(move || control.capture_gate.is_enabled())
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(|_| ApiError::internal())
}
