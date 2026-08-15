use akra_core::{
    ingress::{ActivityKind, IngressEvent, ResultEvent},
    prompt_projection::PromptProjection,
};
use serde::Deserialize;
use serde_json::Value;
use thiserror::Error;

#[path = "codex_lifecycle.rs"]
mod lifecycle;

#[path = "codex_prompt.rs"]
mod prompt;

pub use lifecycle::{
    CodexHookCommand, CodexHookLifecycle, CodexHookLifecycleSet, CodexLifecycleError,
    apply_codex_hook_updates,
};
pub use prompt::project_codex_user_prompt;

#[derive(Debug)]
pub struct CodexAdapter;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CodexCapture {
    Activity(IngressEvent),
    Result(ResultEvent),
}

impl CodexAdapter {
    /// Derives a display/summary input without changing the captured prompt.
    pub fn project_prompt(prompt: &str) -> PromptProjection {
        project_codex_user_prompt(prompt)
    }

    /// Normalizes prompt-producing hooks for callers that predate result capture.
    pub fn normalize(input: &str) -> Result<IngressEvent, CodexAdapterError> {
        let value: Value = serde_json::from_str(input)?;
        match hook_event_name(&value)? {
            "UserPromptSubmit" => normalize_user_prompt(serde_json::from_value(value)?),
            "SubagentStart" => normalize_subagent_start(serde_json::from_value(value)?),
            other => Err(CodexAdapterError::UnexpectedHook(other.to_owned())),
        }
    }

    /// Normalizes every hook consumed by the capture pipeline.
    pub fn normalize_capture(input: &str) -> Result<CodexCapture, CodexAdapterError> {
        let value: Value = serde_json::from_str(input)?;
        match hook_event_name(&value)? {
            "UserPromptSubmit" => {
                normalize_user_prompt(serde_json::from_value(value)?).map(CodexCapture::Activity)
            }
            "SubagentStart" => {
                normalize_subagent_start(serde_json::from_value(value)?).map(CodexCapture::Activity)
            }
            "Stop" => normalize_stop(serde_json::from_value(value)?).map(CodexCapture::Result),
            other => Err(CodexAdapterError::UnexpectedHook(other.to_owned())),
        }
    }
}

fn hook_event_name(value: &Value) -> Result<&str, CodexAdapterError> {
    value
        .get("hook_event_name")
        .and_then(Value::as_str)
        .ok_or(CodexAdapterError::MissingHookName)
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

#[derive(Deserialize)]
struct Stop {
    session_id: String,
    turn_id: String,
    cwd: String,
    model: Option<String>,
    #[serde(default)]
    last_assistant_message: Option<String>,
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

fn normalize_stop(payload: Stop) -> Result<ResultEvent, CodexAdapterError> {
    let result = payload
        .last_assistant_message
        .filter(|message| !message.trim().is_empty());
    ResultEvent::try_new(
        "codex",
        payload.session_id,
        payload.turn_id,
        payload.cwd,
        result,
        payload.model,
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
