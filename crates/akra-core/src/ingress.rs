//! Provider-neutral prompt submission contracts.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProviderId(String);

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    #[default]
    User,
    Subagent,
    Internal,
}

impl ActivityKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Subagent => "subagent",
            Self::Internal => "internal",
        }
    }

    pub fn from_storage(value: &str) -> Option<Self> {
        match value {
            "user" => Some(Self::User),
            "subagent" => Some(Self::Subagent),
            "internal" => Some(Self::Internal),
            _ => None,
        }
    }
}

impl ProviderId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct IngressEvent {
    provider: ProviderId,
    provider_session_id: String,
    provider_turn_id: String,
    cwd: String,
    prompt: String,
    model: Option<String>,
    #[serde(default)]
    activity_kind: ActivityKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_type: Option<String>,
}

impl IngressEvent {
    pub fn try_new(
        provider: impl Into<String>,
        provider_session_id: impl Into<String>,
        provider_turn_id: impl Into<String>,
        cwd: impl Into<String>,
        prompt: impl Into<String>,
        model: Option<String>,
    ) -> Result<Self, IngressError> {
        let provider = provider.into();
        let provider_session_id = provider_session_id.into();
        let provider_turn_id = provider_turn_id.into();
        let cwd = cwd.into();
        let prompt = prompt.into();

        if provider.trim().is_empty() {
            return Err(IngressError::BlankProvider);
        }
        if provider_session_id.trim().is_empty() {
            return Err(IngressError::BlankSessionId);
        }
        if provider_turn_id.trim().is_empty() {
            return Err(IngressError::BlankTurnId);
        }
        if cwd.trim().is_empty() {
            return Err(IngressError::BlankWorkingDirectory);
        }
        if prompt.trim().is_empty() {
            return Err(IngressError::BlankPrompt);
        }

        Ok(Self {
            provider: ProviderId(provider),
            provider_session_id,
            provider_turn_id,
            cwd,
            prompt,
            model,
            activity_kind: ActivityKind::User,
            agent_id: None,
            agent_type: None,
        })
    }

    pub fn with_activity_context(
        mut self,
        activity_kind: ActivityKind,
        agent_id: Option<String>,
        agent_type: Option<String>,
    ) -> Result<Self, IngressError> {
        validate_optional_context("agent id", agent_id.as_deref())?;
        validate_optional_context("agent type", agent_type.as_deref())?;
        if activity_kind != ActivityKind::Subagent && (agent_id.is_some() || agent_type.is_some()) {
            return Err(IngressError::UnexpectedAgentContext);
        }
        self.activity_kind = activity_kind;
        self.agent_id = agent_id;
        self.agent_type = agent_type;
        Ok(self)
    }

    pub fn provider(&self) -> &ProviderId {
        &self.provider
    }

    pub fn prompt(&self) -> &str {
        &self.prompt
    }

    pub fn session_id(&self) -> &str {
        &self.provider_session_id
    }

    pub fn turn_id(&self) -> &str {
        &self.provider_turn_id
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    pub const fn activity_kind(&self) -> ActivityKind {
        self.activity_kind
    }

    pub fn agent_id(&self) -> Option<&str> {
        self.agent_id.as_deref()
    }

    pub fn agent_type(&self) -> Option<&str> {
        self.agent_type.as_deref()
    }
}

fn validate_optional_context(label: &'static str, value: Option<&str>) -> Result<(), IngressError> {
    if let Some(value) = value
        && (value.trim().is_empty() || value.len() > 512 || value.chars().any(char::is_control))
    {
        return Err(IngressError::InvalidActivityContext(label));
    }
    Ok(())
}

#[derive(Debug, Error)]
pub enum IngressError {
    #[error("provider must not be blank")]
    BlankProvider,
    #[error("provider session id must not be blank")]
    BlankSessionId,
    #[error("provider turn id must not be blank")]
    BlankTurnId,
    #[error("working directory must not be blank")]
    BlankWorkingDirectory,
    #[error("prompt must not be blank")]
    BlankPrompt,
    #[error("{0} must be non-blank, at most 512 bytes, and contain no control characters")]
    InvalidActivityContext(&'static str),
    #[error("agent metadata is only valid for subagent activity")]
    UnexpectedAgentContext,
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
