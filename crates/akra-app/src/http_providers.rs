use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};

use crate::{
    codex_targets::{CodexTargetError, CodexTargetSnapshot, CodexTargetStatus},
    collector::{
        CollectorConfigInput, CollectorError, CollectorManager, CollectorMode,
        CollectorStatus as RuntimeCollectorStatus,
    },
    http::{AppState, CodexLifecycleControl},
    http_error::ApiError,
};
use akra_store::PromptSummaryPolicy;

#[derive(Deserialize)]
pub(crate) struct ProviderToggle {
    enabled: bool,
}

#[derive(Serialize)]
pub(crate) struct ProviderStatus {
    provider: String,
    enabled: bool,
    prompt_summary_mode: String,
    targets: Vec<CodexTargetStatus>,
    collector: CollectorIntegration,
}

#[derive(Clone, Serialize)]
pub(crate) struct CollectorIntegration {
    mode: CollectorMode,
    endpoint: String,
    configured: bool,
    token_configured: bool,
    connected: Option<bool>,
    last_delivery_at_us: Option<i64>,
    pending_count: usize,
    last_error: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct CollectorConfiguration {
    endpoint: String,
    #[serde(default)]
    token: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct PromptSummaryConfiguration {
    mode: String,
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
    let collector = collector_integration(state.collector.clone()).await?;
    if let Some(control) = state.codex.clone() {
        return codex_provider_status(provider, control, state.store, collector)
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
        prompt_summary_mode: integration.prompt_summary_mode,
        targets: Vec::new(),
        collector,
    }))
}

pub(crate) async fn configure_prompt_summaries(
    State(state): State<AppState>,
    Json(configuration): Json<PromptSummaryConfiguration>,
) -> Result<StatusCode, ApiError> {
    if let Some(collector) = state.collector.clone() {
        let mode = tokio::task::spawn_blocking(move || {
            collector.status().map(|status| status.config.mode)
        })
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(collector_configuration_error)?;
        if mode == CollectorMode::Remote {
            return Err(ApiError::conflict(
                "prompt_summaries_collector_managed",
                "Prompt summaries are configured on the collector dashboard.",
            ));
        }
    }
    let policy = match configuration.mode.as_str() {
        "off" => PromptSummaryPolicy::Off,
        "smart" => PromptSummaryPolicy::Smart,
        _ => {
            return Err(ApiError::unprocessable(
                "invalid_prompt_summary_mode",
                "Prompt summary mode must be off or smart.",
            ));
        }
    };
    state
        .store
        .set_prompt_summary_policy("codex", policy)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(ApiError::from_store)
}

pub(crate) async fn configure_collector(
    State(state): State<AppState>,
    Json(configuration): Json<CollectorConfiguration>,
) -> Result<StatusCode, ApiError> {
    let collector = state
        .collector
        .ok_or_else(|| ApiError::not_found("Collector settings are unavailable."))?;
    tokio::task::spawn_blocking(move || {
        collector.configure(CollectorConfigInput {
            endpoint: configuration.endpoint,
            token: configuration.token,
        })
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map(|_| StatusCode::NO_CONTENT)
    .map_err(collector_configuration_error)
}

pub(crate) async fn verify_collector(
    State(state): State<AppState>,
) -> Result<StatusCode, ApiError> {
    let collector = state
        .collector
        .ok_or_else(|| ApiError::not_found("Collector settings are unavailable."))?;
    let report = collector
        .verify()
        .await
        .map_err(collector_verification_error)?;
    if report.reachable {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::unprocessable(
            "collector_unreachable",
            "Collector did not accept the configured access token.",
        ))
    }
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
    collector: CollectorIntegration,
) -> Result<ProviderStatus, ApiError> {
    let prompt_summary_mode = store
        .prompt_summary_policy(&provider)
        .await
        .map_err(ApiError::from_store)?
        .as_str()
        .to_owned();
    let observations = store
        .capture_client_observations()
        .await
        .map_err(ApiError::from_store)?;
    tokio::task::spawn_blocking(move || {
        Ok(ProviderStatus {
            provider,
            enabled: control.capture_gate.is_enabled()?,
            prompt_summary_mode,
            targets: control.targets.statuses_with_observations(&observations),
            collector,
        })
    })
    .await
    .map_err(|_| ApiError::internal())?
    .map_err(|_: crate::capture_gate::CaptureGateError| ApiError::internal())
}

async fn collector_integration(
    collector: Option<std::sync::Arc<CollectorManager>>,
) -> Result<CollectorIntegration, ApiError> {
    let Some(collector) = collector else {
        return Ok(default_collector_integration());
    };
    let status = tokio::task::spawn_blocking(move || collector.status())
        .await
        .map_err(|_| ApiError::internal())?
        .map_err(collector_configuration_error)?;
    Ok(collector_integration_from_status(status))
}

fn default_collector_integration() -> CollectorIntegration {
    CollectorIntegration {
        mode: CollectorMode::Local,
        endpoint: crate::collector::DEFAULT_COLLECTOR_ENDPOINT.to_owned(),
        configured: true,
        token_configured: false,
        connected: Some(true),
        last_delivery_at_us: None,
        pending_count: 0,
        last_error: None,
    }
}

fn collector_integration_from_status(status: RuntimeCollectorStatus) -> CollectorIntegration {
    CollectorIntegration {
        mode: status.config.mode,
        endpoint: status.config.endpoint,
        configured: true,
        token_configured: status.config.has_token,
        connected: status.connected,
        last_delivery_at_us: status.last_delivery_at_us,
        pending_count: status.pending,
        last_error: status.last_error,
    }
}

fn collector_configuration_error(error: CollectorError) -> ApiError {
    match error {
        CollectorError::InvalidEndpoint(_)
        | CollectorError::InsecureRemoteEndpoint
        | CollectorError::RemoteTokenRequired
        | CollectorError::InvalidToken
        | CollectorError::InvalidConfig => ApiError::unprocessable(
            "invalid_collector_configuration",
            "Use a loopback http address or an HTTPS collector with an access token.",
        ),
        CollectorError::Http(_) => ApiError::unprocessable(
            "collector_unreachable",
            "Collector could not be reached. Check the address, token, and TLS certificate.",
        ),
        _ => ApiError::internal(),
    }
}

fn collector_verification_error(error: CollectorError) -> ApiError {
    match error {
        CollectorError::Http(_) => ApiError::unprocessable(
            "collector_unreachable",
            "Collector could not be reached. Check the address, token, and TLS certificate.",
        ),
        error => collector_configuration_error(error),
    }
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
