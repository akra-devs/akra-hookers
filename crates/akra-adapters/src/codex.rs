use serde::Deserialize;
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
    pub fn normalize(input: &str) -> Result<akra_core::ingress::IngressEvent, CodexAdapterError> {
        let payload: UserPromptSubmit = serde_json::from_str(input)?;
        if payload.hook_event_name != "UserPromptSubmit" {
            return Err(CodexAdapterError::UnexpectedHook(payload.hook_event_name));
        }
        akra_core::ingress::IngressEvent::try_new(
            "codex",
            payload.session_id,
            payload.turn_id,
            payload.cwd,
            payload.prompt,
            payload.model,
        )
        .map_err(CodexAdapterError::Ingress)
    }
}

#[derive(Deserialize)]
struct UserPromptSubmit {
    hook_event_name: String,
    session_id: String,
    turn_id: String,
    cwd: String,
    prompt: String,
    model: Option<String>,
}

#[derive(Debug, Error)]
pub enum CodexAdapterError {
    #[error("invalid Codex hook payload: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unexpected Codex hook event: {0}")]
    UnexpectedHook(String),
    #[error(transparent)]
    Ingress(#[from] akra_core::ingress::IngressError),
}
