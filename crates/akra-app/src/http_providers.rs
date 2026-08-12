use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    codex_targets::{CodexTargetError, CodexTargetSnapshot, CodexTargetStatus},
    http::{AppState, CodexLifecycleControl},
    http_error::ApiError,
};

#[derive(Deserialize)]
pub(crate) struct ProviderToggle {
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct ProviderStatus {
    provider: String,
    enabled: bool,
    targets: Vec<CodexTargetStatus>,
}

pub(crate) async fn toggle_provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
    Json(toggle): Json<ProviderToggle>,
) -> Result<StatusCode, ApiError> {
    validate_provider(&provider)?;
    let _transition = state.provider_toggle_lock.lock().await;
    let Some(control) = state.codex.clone() else {
        return state
            .store
            .set_provider_enabled(&provider, toggle.enabled)
            .await
            .map(|_| StatusCode::NO_CONTENT)
            .map_err(ApiError::from_store);
    };

    let snapshot = apply_global_target_state(control.clone(), toggle.enabled).await?;
    if state
        .store
        .set_provider_enabled(&provider, toggle.enabled)
        .await
        .is_err()
    {
        let _ = restore_target_state(control, snapshot).await;
        return Err(ApiError::internal());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn toggle_provider_target(
    State(state): State<AppState>,
    Path((provider, target_id)): Path<(String, String)>,
    Json(toggle): Json<ProviderToggle>,
) -> Result<StatusCode, ApiError> {
    validate_provider(&provider)?;
    let _transition = state.provider_toggle_lock.lock().await;
    let control = state
        .codex
        .clone()
        .ok_or_else(|| ApiError::not_found("Codex capture targets are unavailable."))?;
    let rollback_target_id = target_id.clone();
    let (snapshot, aggregate_enabled) =
        apply_individual_target_state(control.clone(), target_id, toggle.enabled).await?;
    if state
        .store
        .set_provider_enabled(&provider, aggregate_enabled)
        .await
        .is_err()
    {
        let _ = restore_individual_target_state(control, rollback_target_id, snapshot).await;
        return Err(ApiError::internal());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn provider(
    State(state): State<AppState>,
    Path(provider): Path<String>,
) -> Result<Json<ProviderStatus>, ApiError> {
    validate_provider(&provider)?;
    let _transition = state.provider_toggle_lock.lock().await;
    if let Some(control) = state.codex {
        return codex_provider_status(provider, control, state.store)
            .await
            .map(Json);
    }
    let integration = state
        .store
        .provider(&provider)
        .await
        .map_err(ApiError::from_store)?;
    Ok(Json(ProviderStatus {
        provider: integration.provider,
        enabled: integration.enabled,
        targets: Vec::new(),
    }))
}

fn validate_provider(provider: &str) -> Result<(), ApiError> {
    if provider == "codex" {
        Ok(())
    } else {
        Err(ApiError::not_found("Provider not found."))
    }
}

async fn codex_provider_status(
    provider: String,
    control: CodexLifecycleControl,
    store: std::sync::Arc<akra_store::ActivityStore>,
) -> Result<ProviderStatus, ApiError> {
    let observations = store
        .capture_client_observations()
        .await
        .map_err(ApiError::from_store)?;
    tokio::task::spawn_blocking(move || {
        Ok(ProviderStatus {
            provider,
            enabled: control.capture_gate.is_enabled()?,
            targets: control.targets.statuses_with_observations(&observations),
        })
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_: crate::capture_gate::CaptureGateError| ApiError::internal())
}

async fn apply_global_target_state(
    control: CodexLifecycleControl,
    enabled: bool,
) -> Result<CodexTargetSnapshot, ApiError> {
    tokio::task::spawn_blocking(move || {
        let snapshot = control.targets.snapshot(&control.capture_gate)?;
        control.targets.apply_all(&control.capture_gate, enabled)?;
        Ok::<_, CodexTargetError>(snapshot)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(target_error)
}

async fn apply_individual_target_state(
    control: CodexLifecycleControl,
    target_id: String,
    enabled: bool,
) -> Result<(CodexTargetSnapshot, bool), ApiError> {
    tokio::task::spawn_blocking(move || {
        let snapshot = control
            .targets
            .snapshot_target(&control.capture_gate, &target_id)?;
        control
            .targets
            .apply_target(&control.capture_gate, &target_id, enabled)?;
        let aggregate_enabled = control.capture_gate.is_enabled()?;
        Ok::<_, CodexTargetError>((snapshot, aggregate_enabled))
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(target_error)
}

async fn restore_target_state(
    control: CodexLifecycleControl,
    snapshot: CodexTargetSnapshot,
) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || control.targets.restore(&control.capture_gate, &snapshot))
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(target_error)
}

async fn restore_individual_target_state(
    control: CodexLifecycleControl,
    target_id: String,
    snapshot: CodexTargetSnapshot,
) -> Result<(), ApiError> {
    tokio::task::spawn_blocking(move || {
        control
            .targets
            .restore_target(&control.capture_gate, &target_id, &snapshot)
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(target_error)
}

fn target_error(error: CodexTargetError) -> ApiError {
    match error {
        CodexTargetError::UnknownTarget(_) => ApiError::not_found(error.to_string()),
        CodexTargetError::NoAvailableTargets | CodexTargetError::UnavailableTarget { .. } => {
            ApiError::unprocessable("codex_target_unavailable", error.to_string())
        }
        CodexTargetError::TargetLifecycle { .. } => {
            ApiError::unprocessable("codex_hook_update_failed", error.to_string())
        }
        CodexTargetError::Gate(_) | CodexTargetError::Rollback { .. } => ApiError::internal(),
    }
}
