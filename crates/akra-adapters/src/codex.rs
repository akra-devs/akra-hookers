use akra_core::ingress::{ActivityKind, IngressEvent};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

#[path = "codex_lifecycle.rs"]
mod lifecycle;

pub use lifecycle::{
    CodexHookCommand, CodexHookLifecycle, CodexHookLifecycleSet, CodexLifecycleError,
    apply_codex_hook_updates,
};

#[derive(Debug)]
pub struct CodexAdapter;

impl CodexAdapter {
    pub fn normalize(input: &str) -> Result<IngressEvent, CodexAdapterError> {
        let value: Value = serde_json::from_str(input)?;
        let hook_event_name = value
            .get("hook_event_name")
            .and_then(Value::as_str)
            .ok_or(CodexAdapterError::MissingHookName)?;
        match hook_event_name {
            "UserPromptSubmit" => normalize_user_prompt(serde_json::from_value(value)?),
            "SubagentStart" => normalize_subagent_start(serde_json::from_value(value)?),
            other => Err(CodexAdapterError::UnexpectedHook(other.to_owned())),
        }
    }
}

#[derive(Deserialize)]
struct UserPromptSubmit {
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
    model: Option<String>,
}

#[derive(Deserialize)]
struct SubagentStart {
    session_id: String,
    turn_id: String,
    cwd: String,
    model: Option<String>,
    agent_id: String,
    agent_type: String,
}

fn normalize_user_prompt(payload: UserPromptSubmit) -> Result<IngressEvent, CodexAdapterError> {
    IngressEvent::try_new(
        "codex",
        payload.session_id,
        payload.turn_id,
        payload.cwd,
        payload.prompt,
        payload.model,
    )
    .map_err(CodexAdapterError::Ingress)
}

fn normalize_subagent_start(payload: SubagentStart) -> Result<IngressEvent, CodexAdapterError> {
    let turn_id = format!("{}:subagent:{}", payload.turn_id, payload.agent_id);
    let prompt = format!("Subagent started: {}", payload.agent_type);
    IngressEvent::try_new(
        "codex",
        payload.session_id,
        turn_id,
        payload.cwd,
        prompt,
        payload.model,
    )?
    .with_activity_context(
        ActivityKind::Subagent,
        Some(payload.agent_id),
        Some(payload.agent_type),
    )
    .map_err(CodexAdapterError::Ingress)
}

#[derive(Debug, Error)]
pub enum CodexAdapterError {
    #[error("invalid Codex hook payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Codex hook payload is missing hook_event_name")]
    MissingHookName,
    #[error("unexpected Codex hook event: {0}")]
    UnexpectedHook(String),
    #[error(transparent)]
    Ingress(#[from] akra_core::ingress::IngressError),
}
