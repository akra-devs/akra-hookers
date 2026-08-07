//! Provider-neutral prompt submission contracts.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(transparent)]
pub struct ProviderId(String);

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
        })
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
}

impl fmt::Display for ProviderId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(formatter)
    }
}
