use std::{
    fs::{self, File},
    io::{BufRead, BufReader, Read},
    path::{Component, Path, PathBuf},
};

use serde_json::Value;

const MAX_SESSION_META_BYTES: u64 = 64 * 1024;

/// Best-effort runtime classification used only for capture diagnostics.
/// Capture correctness never depends on the transcript format remaining stable.
pub fn codex_client(payload: &Value, wsl_distro: Option<&str>) -> &'static str {
    if wsl_distro.is_some() {
        return "wsl_cli";
    }

    payload
        .get("transcript_path")
        .and_then(Value::as_str)
        .and_then(session_meta)
        .and_then(|value| classify_session_meta(&value))
        .unwrap_or("unknown")
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

fn classify_session_meta(value: &Value) -> Option<&'static str> {
    let payload = value
        .get("payload")
        .filter(|_| value.get("type").and_then(Value::as_str) == Some("session_meta"))?;
    let originator = payload
        .get("originator")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let source = payload
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_ascii_lowercase();

    if originator.contains("desktop") || source == "vscode" {
        Some("app")
    } else if originator.contains("exec")
        || source == "exec"
        || originator == "codex-tui"
        || originator.contains("codex_cli")
        || source == "cli"
    {
        Some("cli")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::classify_session_meta;

    #[test]
    fn classifies_desktop_and_cli_session_metadata() {
        assert_eq!(
            classify_session_meta(&json!({
                "type": "session_meta",
                "payload": { "originator": "Codex Desktop", "source": "vscode" }
            })),
            Some("app")
        );
        assert_eq!(
            classify_session_meta(&json!({
                "type": "session_meta",
                "payload": { "originator": "codex-tui", "source": "cli" }
            })),
            Some("cli")
        );
    }

    #[test]
    fn unknown_metadata_stays_diagnostic_only() {
        assert_eq!(
            classify_session_meta(&json!({
                "type": "session_meta",
                "payload": { "originator": "future-client" }
            })),
            None
        );
    }
}
