use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
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
            manifest_path: home.join(".codex").join("hooks.json"),
        }
    }

    pub fn enable(&self, command: &str) -> Result<(), CodexLifecycleError> {
        let parent = self
            .manifest_path
            .parent()
            .ok_or(CodexLifecycleError::MissingManifestParent)?;
        fs::create_dir_all(parent)?;
        let mut hooks = self.read_hooks()?;
        for group in &mut hooks.hooks.user_prompt_submit {
            group.hooks.retain(|hook| !hook.is_akra_hook());
        }
        hooks
            .hooks
            .user_prompt_submit
            .retain(|group| !group.hooks.is_empty());
        hooks
            .hooks
            .user_prompt_submit
            .push(CodexMatcherGroup::akra_hook(command));
        self.write_hooks(&hooks)?;
        Ok(())
    }

    pub fn disable(&self) -> Result<(), CodexLifecycleError> {
        if self.manifest_path.exists() {
            let mut hooks = self.read_hooks()?;
            for group in &mut hooks.hooks.user_prompt_submit {
                group.hooks.retain(|hook| !hook.is_akra_hook());
            }
            hooks
                .hooks
                .user_prompt_submit
                .retain(|group| !group.hooks.is_empty());
            self.write_hooks(&hooks)?;
        }
        Ok(())
    }

    pub fn is_enabled(&self) -> Result<bool, CodexLifecycleError> {
        let hooks = match self.read_hooks() {
            Ok(hooks) => hooks,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(CodexLifecycleError::Io(error)),
        };
        Ok(hooks
            .hooks
            .user_prompt_submit
            .iter()
            .flat_map(|group| &group.hooks)
            .any(CodexHook::is_akra_hook))
    }

    fn read_hooks(&self) -> Result<CodexHooksFile, std::io::Error> {
        match fs::read_to_string(&self.manifest_path) {
            Ok(content) => serde_json::from_str(&content).map_err(std::io::Error::other),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(CodexHooksFile::default())
            }
            Err(error) => Err(error),
        }
    }

    fn write_hooks(&self, hooks: &CodexHooksFile) -> Result<(), CodexLifecycleError> {
        fs::write(&self.manifest_path, serde_json::to_string_pretty(hooks)?)?;
        Ok(())
    }
}

#[derive(Default, Deserialize, Serialize)]
struct CodexHooksFile {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    description: Option<String>,
    #[serde(default)]
    hooks: CodexHookEvents,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Default, Deserialize, Serialize)]
struct CodexHookEvents {
    #[serde(rename = "UserPromptSubmit", default)]
    user_prompt_submit: Vec<CodexMatcherGroup>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

#[derive(Deserialize, Serialize)]
struct CodexMatcherGroup {
    #[serde(default)]
    hooks: Vec<CodexHook>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl CodexMatcherGroup {
    fn akra_hook(command: &str) -> Self {
        Self {
            hooks: vec![CodexHook {
                hook_type: "command".to_owned(),
                command: command.to_owned(),
                asynchronous: Some(true),
                extra: BTreeMap::new(),
            }],
            extra: BTreeMap::new(),
        }
    }
}

#[derive(Deserialize, Serialize)]
struct CodexHook {
    #[serde(rename = "type")]
    hook_type: String,
    command: String,
    #[serde(default, rename = "async", skip_serializing_if = "Option::is_none")]
    asynchronous: Option<bool>,
    #[serde(flatten)]
    extra: BTreeMap<String, serde_json::Value>,
}

impl CodexHook {
    fn is_akra_hook(&self) -> bool {
        self.hook_type == "command"
            && self.command.contains("akra-hookers")
            && self.command.contains(" capture")
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
    #[error("Codex lifecycle serialization failed: {0}")]
    Json(#[from] serde_json::Error),
}
