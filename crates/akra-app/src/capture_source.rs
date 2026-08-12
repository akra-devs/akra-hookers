use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

use akra_core::ingress::ActivityKind;

const MAX_SESSION_META_BYTES: u64 = 64 * 1024;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexCaptureContext {
    pub client: &'static str,
    pub activity_kind: ActivityKind,
    pub agent_id: Option<String>,
    pub agent_type: Option<String>,
}

/// Best-effort runtime classification for capture provenance and canvas visibility.
/// The hook event is authoritative for subagents; session metadata and the Codex
/// application directory are conservative fallbacks for older prompt captures.
pub fn codex_capture_context(payload: &Value, wsl_distro: Option<&str>) -> CodexCaptureContext {
    let meta = payload
        .get("transcript_path")
        .and_then(Value::as_str)
        .and_then(session_meta)
        .and_then(|value| classify_session_meta(&value));
    let client = if wsl_distro.is_some() {
        "wsl_cli"
    } else {
        meta.as_ref()
            .and_then(|classification| classification.client)
            .unwrap_or("unknown")
    };

    if payload.get("hook_event_name").and_then(Value::as_str) == Some("SubagentStart") {
        return CodexCaptureContext {
            client,
            activity_kind: ActivityKind::Subagent,
            agent_id: payload
                .get("agent_id")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
            agent_type: payload
                .get("agent_type")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned),
        };
    }

    if let Some(classification) = meta
        && classification.activity_kind != ActivityKind::User
    {
        return CodexCaptureContext {
            client,
            activity_kind: classification.activity_kind,
            agent_id: classification.agent_id,
            agent_type: classification.agent_type,
        };
    }

    let activity_kind = payload
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| is_codex_internal_cwd(cwd))
        .map_or(ActivityKind::User, |_| ActivityKind::Internal);
    CodexCaptureContext {
        client,
        activity_kind,
        agent_id: None,
        agent_type: None,
    }
}

pub fn codex_managed_capture_context(
    payload: &Value,
    wsl_distro: Option<&str>,
) -> CodexCaptureContext {
    let mut context = codex_capture_context(payload, wsl_distro);
    if context.client == "unknown" && context.activity_kind == ActivityKind::User {
        context.activity_kind = ActivityKind::Internal;
    }
    context
}

pub fn codex_client(payload: &Value, wsl_distro: Option<&str>) -> &'static str {
    codex_capture_context(payload, wsl_distro).client
}

fn session_meta(transcript_path: &str) -> Option<Value> {
    let path = PathBuf::from(transcript_path);
    if !is_codex_session_path(&path) {
        return None;
    }
    let metadata = fs::symlink_metadata(&path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return None;
    }
    let file = File::open(path).ok()?;
    let mut line = String::new();
    BufReader::new(file)
        .take(MAX_SESSION_META_BYTES)
        .read_line(&mut line)
        .ok()?;
    serde_json::from_str(&line).ok()
}

fn is_codex_session_path(path: &Path) -> bool {
    let components = path
        .components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .map(str::to_ascii_lowercase)
        .collect::<Vec<_>>();
    components
        .windows(2)
        .any(|pair| pair[0] == ".codex" && pair[1] == "sessions")
        && path
            .extension()
            .is_some_and(|extension| extension == "jsonl")
}

#[derive(Debug)]
struct SessionClassification {
    client: Option<&'static str>,
    activity_kind: ActivityKind,
    agent_id: Option<String>,
    agent_type: Option<String>,
}

fn classify_session_meta(value: &Value) -> Option<SessionClassification> {
    let payload = value
        .get("payload")
        .filter(|_| value.get("type").and_then(Value::as_str) == Some("session_meta"))?;
    let originator = payload
        .get("originator")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let source = payload.get("source");
    let source_name = source
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let client = if originator.contains("desktop") || source_name == "vscode" {
        Some("app")
    } else if originator.contains("exec")
        || source_name == "exec"
        || originator == "codex-tui"
        || originator.contains("codex_cli")
        || source_name == "cli"
    {
        Some("cli")
    } else {
        None
    };
    let thread_source = payload
        .get("thread_source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let subagent = source.and_then(|source| source.get("subagent"));
    let internal = source.is_some_and(|source| {
        source.get("ambient").is_some()
            || source.get("ambient_suggestion").is_some()
            || source.get("internal").is_some()
    });
    let activity_kind = if thread_source == "subagent" || subagent.is_some() {
        ActivityKind::Subagent
    } else if matches!(thread_source.as_str(), "internal" | "system") || internal {
        ActivityKind::Internal
    } else {
        ActivityKind::User
    };
    let spawn = subagent.and_then(|value| value.get("thread_spawn"));
    Some(SessionClassification {
        client,
        activity_kind,
        agent_id: payload
            .get("id")
            .or_else(|| payload.get("agent_id"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
        agent_type: spawn
            .and_then(|value| {
                value
                    .get("agent_type")
                    .or_else(|| value.get("agent_nickname"))
            })
            .or_else(|| payload.get("agent_type"))
            .and_then(Value::as_str)
            .map(ToOwned::to_owned),
    })
}

fn is_codex_internal_cwd(cwd: &str) -> bool {
    let normalized = cwd.replace('\\', "/").to_ascii_lowercase();
    normalized.contains("/windowsapps/openai.codex_") && normalized.ends_with("/app")
}

#[cfg(test)]
mod tests {
    use akra_core::ingress::ActivityKind;
    use serde_json::json;

    use super::{
        classify_session_meta, codex_capture_context, codex_managed_capture_context,
        is_codex_internal_cwd,
    };

    #[test]
    fn classifies_desktop_and_cli_session_metadata() {
        assert_eq!(
            classify_session_meta(&json!({
                "type": "session_meta",
                "payload": { "originator": "Codex Desktop", "source": "vscode" }
            }))
            .and_then(|value| value.client),
            Some("app")
        );
        assert_eq!(
            classify_session_meta(&json!({
                "type": "session_meta",
                "payload": { "originator": "codex-tui", "source": "cli" }
            }))
            .and_then(|value| value.client),
            Some("cli")
        );
    }

    #[test]
    fn classifies_subagent_metadata_with_official_identity() {
        let classification = classify_session_meta(&json!({
            "type": "session_meta",
            "payload": {
                "id": "agent-thread-7",
                "originator": "Codex Desktop",
                "thread_source": "subagent",
                "source": { "subagent": { "thread_spawn": { "agent_nickname": "reviewer" } } }
            }
        }))
        .expect("session metadata");

        assert_eq!(classification.client, Some("app"));
        assert_eq!(classification.activity_kind, ActivityKind::Subagent);
        assert_eq!(classification.agent_id.as_deref(), Some("agent-thread-7"));
        assert_eq!(classification.agent_type.as_deref(), Some("reviewer"));
    }

    #[test]
    fn hook_event_and_codex_app_directory_classify_without_prompt_matching() {
        let subagent = codex_capture_context(
            &json!({
                "hook_event_name": "SubagentStart",
                "agent_id": "agent-7",
                "agent_type": "reviewer"
            }),
            None,
        );
        assert_eq!(subagent.activity_kind, ActivityKind::Subagent);
        assert_eq!(subagent.agent_id.as_deref(), Some("agent-7"));

        let cwd = r"C:\Program Files\WindowsApps\OpenAI.Codex_1.0.0_x64__8wekyb3d8bbwe\app";
        assert!(is_codex_internal_cwd(cwd));
        let internal = codex_capture_context(
            &json!({ "hook_event_name": "UserPromptSubmit", "cwd": cwd }),
            None,
        );
        assert_eq!(internal.activity_kind, ActivityKind::Internal);
    }

    #[test]
    fn unknown_metadata_remains_user_activity() {
        let classification = classify_session_meta(&json!({
            "type": "session_meta",
            "payload": { "originator": "future-client", "thread_source": "user" }
        }))
        .expect("session metadata");
        assert_eq!(classification.client, None);
        assert_eq!(classification.activity_kind, ActivityKind::User);
    }

    #[test]
    fn managed_capture_treats_unattributed_ephemeral_sessions_as_internal() {
        let context = codex_managed_capture_context(
            &json!({
                "hook_event_name": "UserPromptSubmit",
                "cwd": "C:/dev/project"
            }),
            None,
        );
        assert_eq!(context.client, "unknown");
        assert_eq!(context.activity_kind, ActivityKind::Internal);

        let manual = codex_capture_context(
            &json!({
                "hook_event_name": "UserPromptSubmit",
                "cwd": "C:/dev/project"
            }),
            None,
        );
        assert_eq!(manual.activity_kind, ActivityKind::User);
    }
}
