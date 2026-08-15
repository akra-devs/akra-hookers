//! Derived, provider-neutral prompt display input.
//!
//! A projection never replaces the captured ingress prompt.  It is a
//! separately stored, versioned representation that may remove provider UI
//! wrappers before a later summary job is considered.

use serde::{Deserialize, Serialize};

/// Version of the conservative Codex prompt projection contract.
pub const CODEX_PROMPT_PROJECTION_VERSION: i64 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct PromptProjection {
    text: String,
    kind: PromptProjectionKind,
    removed_chars: usize,
    version: i64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptProjectionKind {
    Raw,
    CodexWrapperRemoved,
}

impl PromptProjection {
    /// Creates the no-op projection used whenever a provider wrapper cannot be
    /// recognized completely and conservatively.
    pub fn raw(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            kind: PromptProjectionKind::Raw,
            removed_chars: 0,
            version: CODEX_PROMPT_PROJECTION_VERSION,
        }
    }

    /// Creates a projection after a provider-specific parser has removed only
    /// canonical wrapper content.  Callers must fall back to [`Self::raw`] if
    /// `text` is blank or if the source shape was not fully recognized.
    pub fn codex_wrapper_removed(text: impl Into<String>, removed_chars: usize) -> Option<Self> {
        let text = text.into();
        (!text.trim().is_empty()).then_some(Self {
            text,
            kind: PromptProjectionKind::CodexWrapperRemoved,
            removed_chars,
            version: CODEX_PROMPT_PROJECTION_VERSION,
        })
    }

    /// Restores a persisted derived input.  This deliberately has no provider
    /// parsing behavior; callers must have already stored a validated
    /// projection kind and version.
    pub fn restored(
        text: impl Into<String>,
        kind: PromptProjectionKind,
        version: i64,
    ) -> Option<Self> {
        let text = text.into();
        (version > 0 && !text.trim().is_empty()).then_some(Self {
            text,
            kind,
            removed_chars: 0,
            version,
        })
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    pub const fn kind(&self) -> PromptProjectionKind {
        self.kind
    }

    pub const fn removed_chars(&self) -> usize {
        self.removed_chars
    }

    pub const fn version(&self) -> i64 {
        self.version
    }
}

#[cfg(test)]
mod tests {
    use super::{CODEX_PROMPT_PROJECTION_VERSION, PromptProjection, PromptProjectionKind};

    #[test]
    fn raw_projection_preserves_the_input_verbatim() {
        let projection = PromptProjection::raw("  진행해\r\n");

        assert_eq!(projection.text(), "  진행해\r\n");
        assert_eq!(projection.kind(), PromptProjectionKind::Raw);
        assert_eq!(projection.removed_chars(), 0);
        assert_eq!(projection.version(), CODEX_PROMPT_PROJECTION_VERSION);
    }

    #[test]
    fn removed_projection_requires_visible_text() {
        assert!(PromptProjection::codex_wrapper_removed(" \n", 3).is_none());

        let projection = PromptProjection::codex_wrapper_removed("진행해", 12).expect("text");
        assert_eq!(projection.kind(), PromptProjectionKind::CodexWrapperRemoved);
        assert_eq!(projection.removed_chars(), 12);
    }
}
