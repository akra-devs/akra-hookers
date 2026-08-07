use std::{
    fs,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use thiserror::Error;

#[derive(Debug)]
pub struct CodexAdapter;

#[derive(Debug)]
pub struct CodexHookLifecycle {
    manifest_path: PathBuf,
}

impl CodexHookLifecycle {
    pub fn new(home: &Path) -> Self {
        Self {
            manifest_path: home.join(".codex").join("akra-hookers-hook.json"),
        }
    }

    pub fn enable(&self, command: &str) -> Result<(), CodexLifecycleError> {
        let parent = self
            .manifest_path
            .parent()
            .ok_or(CodexLifecycleError::MissingManifestParent)?;
        fs::create_dir_all(parent)?;
        fs::write(
            &self.manifest_path,
            serde_json::json!({
                "hook_event_name": "UserPromptSubmit",
                "command": command,
                "managed_by": "akra-hookers"
            })
            .to_string(),
        )?;
        Ok(())
    }

    pub fn disable(&self) -> Result<(), CodexLifecycleError> {
        if self.manifest_path.exists() {
            fs::write(&self.manifest_path, "{}")?;
        }
        Ok(())
    }

    pub fn is_enabled(&self) -> Result<bool, CodexLifecycleError> {
        let content = match fs::read_to_string(&self.manifest_path) {
            Ok(content) => content,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error.into()),
        };
        Ok(content.contains("\"UserPromptSubmit\""))
    }
}

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

#[derive(Debug, Error)]
pub enum CodexLifecycleError {
    #[error("Codex hook manifest has no parent directory")]
    MissingManifestParent,
    #[error("Codex lifecycle filesystem operation failed: {0}")]
    Io(#[from] std::io::Error),
}
