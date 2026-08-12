use std::{fs, path::Path};

use akra_adapters::codex::{CodexHookLifecycle, CodexHookLifecycleSet};
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
                    }, {
                        "type": "command",
                        "command": "wrapper akra-hookers capture --data-dir C:\\foreign"
                    }, {
                        "type": "command",
                        "command": "echo C:\\tools\\akra-hookers.exe capture --data-dir C:\\foreign"
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
            .contains(&"wrapper akra-hookers capture --data-dir C:\\foreign".to_owned())
    );
    assert!(
        enabled_commands.contains(
            &"echo C:\\tools\\akra-hookers.exe capture --data-dir C:\\foreign".to_owned()
        )
    );
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
    assert_eq!(
        enabled["hooks"]["UserPromptSubmit"][1]["hooks"][0]["akraHookersManaged"],
        true
    );

    lifecycle.disable().expect("disable");
    assert!(!lifecycle.is_enabled().expect("status"));
    assert_eq!(
        commands(&read_hooks(&hooks_path)),
        vec![
            "keep-existing-hook",
            "wrapper akra-hookers capture --data-dir C:\\foreign",
            "echo C:\\tools\\akra-hookers.exe capture --data-dir C:\\foreign"
        ]
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
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]["commandWindows"],
        "C:\\tools\\akra-hookers.exe capture --data-dir C:\\data",
        "Codex requires an explicit Windows command override"
    );
    assert!(
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]
            .get("async")
            .is_none(),
        "akra capture must be synchronous because Codex skips async hooks"
    );

    let config =
        fs::read_to_string(home.path().join(".codex").join("config.toml")).expect("trusted config");
    assert!(config.contains("enabled = true"));
    assert!(
        config.contains("sha256:e94def78b62a7838e51bc8b77e885b5e85c89162fc063bc9a3c4cfd4c8237f36")
    );
}

#[test]
fn enable_is_idempotent_for_dashboard_repeated_on_actions() {
    let home = TempDir::new().expect("home directory");
    let lifecycle = CodexHookLifecycle::new(home.path());

    lifecycle
        .enable(r#"C:\tools\akra-hookers.exe capture --data-dir C:\data"#)
        .expect("first enable");
    let config_path = home.path().join(".codex").join("config.toml");
    let first_config = fs::read(&config_path).expect("first config");
    lifecycle
        .enable(r#"C:\tools\akra-hookers.exe capture --data-dir C:\data"#)
        .expect("second enable");

    let hooks = read_hooks(&home.path().join(".codex").join("hooks.json"));
    assert_eq!(
        commands(&hooks),
        vec![r#"C:\tools\akra-hookers.exe capture --data-dir C:\data"#]
    );
    assert_eq!(
        fs::read(config_path).expect("second config"),
        first_config,
        "repeated enable must not churn Codex trust state"
    );
}

#[test]
fn enable_preserves_unrelated_config_and_disable_removes_only_akra_trust() {
    let home = TempDir::new().expect("home directory");
    let codex_home = home.path().join(".codex");
    fs::create_dir_all(&codex_home).expect("Codex home");
    let config_path = codex_home.join("config.toml");
    fs::write(
        &config_path,
        r#"# retain this comment
model = "gpt-test"

[hooks.state.'other.json:user_prompt_submit:0:0']
enabled = false
trusted_hash = "sha256:other"
"#,
    )
    .expect("existing config");
    let lifecycle = CodexHookLifecycle::new(home.path());

    lifecycle
        .enable("akra-hookers capture")
        .expect("enable and trust Akra hook");
    let enabled = fs::read_to_string(&config_path).expect("enabled config");
    assert!(enabled.contains("# retain this comment"));
    assert!(enabled.contains("model = \"gpt-test\""));
    assert!(enabled.contains("sha256:other"));
    assert!(
        enabled.contains("sha256:bd757851234b867d380008403ac0e54873ab8ff31dc399fae293dd2d5362a26b")
    );

    lifecycle.disable().expect("disable Akra hook");
    let disabled = fs::read_to_string(config_path).expect("disabled config");
    assert!(disabled.contains("# retain this comment"));
    assert!(disabled.contains("sha256:other"));
    assert!(
        !disabled
            .contains("sha256:bd757851234b867d380008403ac0e54873ab8ff31dc399fae293dd2d5362a26b")
    );
}

#[test]
fn malformed_config_aborts_before_manifest_mutation() {
    let home = TempDir::new().expect("home directory");
    let manifest = home.path().join("hooks.json");
    let config = home.path().join("config.toml");
    let original = br#"{ "description": "preserve exact bytes", "hooks": {} }
"#;
    fs::write(&manifest, original).expect("manifest");
    fs::write(&config, b"[hooks.state\n").expect("malformed config");
    let lifecycle = CodexHookLifecycle::from_codex_home(home.path());

    let error = lifecycle
        .enable("akra-hookers capture")
        .expect_err("malformed config must abort installation");

    assert!(error.to_string().contains("not valid TOML"));
    assert_eq!(fs::read(manifest).expect("unchanged manifest"), original);
    assert_eq!(
        fs::read(config).expect("unchanged config"),
        b"[hooks.state\n"
    );
}

#[test]
fn enable_replaces_a_stale_managed_command_with_the_current_synchronous_command() {
    let home = TempDir::new().expect("home directory");
    let lifecycle = CodexHookLifecycle::new(home.path());
    let stale = r#"C:\old\akra-hookers.exe capture --data-dir C:\old-data"#;
    let current = r#"C:\current\akra-hookers.exe capture --data-dir C:\current-data"#;

    lifecycle.enable(stale).expect("stale hook");
    lifecycle.enable(current).expect("current hook");

    let hooks = read_hooks(&home.path().join(".codex").join("hooks.json"));
    assert_eq!(commands(&hooks), vec![current]);
    assert!(
        hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0]
            .get("async")
            .is_none(),
        "akra capture must not use Codex-unsupported async hooks"
    );
}

#[test]
fn enable_does_not_change_an_earlier_home_when_a_later_manifest_is_malformed() {
    let homes = TempDir::new().expect("homes directory");
    let first_home = homes.path().join("first").join(".codex");
    let second_home = homes.path().join("second").join(".codex");
    let first_manifest = first_home.join("hooks.json");
    let second_manifest = second_home.join("hooks.json");
    fs::create_dir_all(&first_home).expect("first Codex home");
    fs::create_dir_all(&second_home).expect("second Codex home");
    let original = br#"{ "description": "preserve these exact bytes", "hooks": {} }
"#;
    fs::write(&first_manifest, original).expect("first manifest");
    fs::write(&second_manifest, b"{ malformed").expect("malformed second manifest");
    let lifecycle = CodexHookLifecycleSet::from_codex_homes([first_home, second_home]);

    lifecycle
        .enable(r#"C:\tools\akra-hookers.exe capture --data-dir C:\data"#)
        .expect_err("malformed later manifest must fail enable");

    assert_eq!(
        fs::read(first_manifest).expect("unchanged first manifest"),
        original
    );
}

#[test]
fn disable_does_not_change_an_earlier_home_when_a_later_manifest_is_malformed() {
    let homes = TempDir::new().expect("homes directory");
    let first_home = homes.path().join("first").join(".codex");
    let second_home = homes.path().join("second").join(".codex");
    let first_manifest = first_home.join("hooks.json");
    let second_manifest = second_home.join("hooks.json");
    fs::create_dir_all(&first_home).expect("first Codex home");
    fs::create_dir_all(&second_home).expect("second Codex home");
    let original = br#"{
  "description": "preserve formatting too",
  "hooks": { "UserPromptSubmit": [{ "hooks": [{ "type": "command", "command": "akra-hookers capture" }] }] }
}
"#;
    fs::write(&first_manifest, original).expect("first manifest");
    fs::write(&second_manifest, b"not json").expect("malformed second manifest");
    let lifecycle = CodexHookLifecycleSet::from_codex_homes([first_home, second_home]);

    lifecycle
        .disable()
        .expect_err("malformed later manifest must fail disable");

    assert_eq!(
        fs::read(first_manifest).expect("unchanged first manifest"),
        original
    );
}

#[test]
fn oversized_manifest_is_rejected_without_mutation() {
    let home = TempDir::new().expect("Codex home");
    let manifest = home.path().join("hooks.json");
    let original = vec![b' '; 1024 * 1024 + 1];
    fs::write(&manifest, &original).expect("oversized manifest");
    let lifecycle = CodexHookLifecycle::from_codex_home(home.path());

    let error = lifecycle
        .enable("akra-hookers capture")
        .expect_err("oversized manifest must fail");

    assert!(error.to_string().contains("exceeding"));
    assert_eq!(fs::read(manifest).expect("unchanged manifest"), original);
}

#[test]
fn non_regular_manifest_is_rejected_without_replacement() {
    let home = TempDir::new().expect("Codex home");
    let manifest = home.path().join("hooks.json");
    fs::create_dir(&manifest).expect("manifest directory");
    let lifecycle = CodexHookLifecycle::from_codex_home(home.path());

    let error = lifecycle
        .enable("akra-hookers capture")
        .expect_err("non-regular manifest must fail");

    assert!(error.to_string().contains("regular non-link file"));
    assert!(manifest.is_dir());
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
