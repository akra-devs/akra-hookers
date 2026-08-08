use std::{fs, path::Path};

use akra_adapters::codex::CodexHookLifecycle;
use serde_json::json;
use tempfile::TempDir;

#[test]
fn enable_then_disable_manages_only_akra_hook_entry() {
    let home = TempDir::new().expect("temp home");
    let lifecycle = CodexHookLifecycle::new(home.path());
    let hooks_path = home.path().join(".codex").join("hooks.json");
    fs::create_dir_all(hooks_path.parent().expect("parent")).expect("config directory");
    fs::write(
        &hooks_path,
        json!({
            "description": "existing hooks",
            "hooks": {
                "UserPromptSubmit": [{
                    "matcher": ".*",
                    "hooks": [{
                        "type": "command",
                        "command": "keep-existing-hook",
                        "async": true
                    }]
                }]
            }
        })
        .to_string(),
    )
    .expect("existing hooks");

    lifecycle
        .enable("C:\\tools\\akra-hookers.exe capture --data-dir C:\\data")
        .expect("enable");
    assert!(lifecycle.is_enabled().expect("status"));

    let enabled = read_hooks(&hooks_path);
    let enabled_commands = commands(&enabled);
    assert!(enabled_commands.contains(&"keep-existing-hook".to_owned()));
    assert!(
        enabled_commands
            .contains(&"C:\\tools\\akra-hookers.exe capture --data-dir C:\\data".to_owned())
    );
    assert_eq!(enabled["description"], "existing hooks");
    assert_eq!(enabled["hooks"]["UserPromptSubmit"][0]["matcher"], ".*");
    assert_eq!(
        enabled["hooks"]["UserPromptSubmit"][0]["hooks"][0]["async"],
        true
    );

    lifecycle.disable().expect("disable");
    assert!(!lifecycle.is_enabled().expect("status"));
    assert_eq!(
        commands(&read_hooks(&hooks_path)),
        vec!["keep-existing-hook"]
    );
}

#[test]
fn enable_creates_missing_hooks_configuration() {
    let home = TempDir::new().expect("temp home");
    let lifecycle = CodexHookLifecycle::new(home.path());

    lifecycle
        .enable("C:\\tools\\akra-hookers.exe capture --data-dir C:\\data")
        .expect("enable");

    let hooks = read_hooks(&home.path().join(".codex").join("hooks.json"));
    assert_eq!(
        commands(&hooks),
        vec!["C:\\tools\\akra-hookers.exe capture --data-dir C:\\data"]
    );
    assert_eq!(
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["async"],
        true
    );
}

fn read_hooks(path: &Path) -> serde_json::Value {
    serde_json::from_str(&fs::read_to_string(path).expect("hooks")).expect("valid hooks json")
}

fn commands(value: &serde_json::Value) -> Vec<String> {
    value["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("prompt submit groups")
        .iter()
        .flat_map(|group| group["hooks"].as_array().expect("group hooks"))
        .filter_map(|hook| hook["command"].as_str())
        .map(ToOwned::to_owned)
        .collect()
}
