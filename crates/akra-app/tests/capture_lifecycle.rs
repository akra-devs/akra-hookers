use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use akra_app::spool::{MAX_CAPTURE_INPUT_BYTES, Spool};
use tempfile::TempDir;

#[test]
fn capture_spools_without_opening_sqlite_or_printing_the_prompt() {
    let data_dir = TempDir::new().expect("data directory");

    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("capture starts");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"session","turn_id":"turn","cwd":"project","prompt":"retain only in spool","model":"test"}"#,
        )
        .expect("payload writes");

    let output = child.wait_with_output().expect("capture exits");
    assert!(output.status.success(), "capture failed: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "hook output must not expose prompt content"
    );
    assert!(
        !data_dir.path().join("akra-hookers.sqlite").exists(),
        "fast capture must not open SQLite or execute migrations"
    );
    assert_eq!(
        fs::read_dir(data_dir.path().join("spool"))
            .expect("spool directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "pending")
            })
            .count(),
        1,
        "validated hook input must produce one durable spool item"
    );
    let spool = Spool::open(&data_dir.path().join("spool")).expect("spool");
    let pending = spool.pending().expect("pending");
    assert_eq!(pending.len(), 1);
    let bytes = spool.read(&pending[0]).expect("pending payload");
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).expect("versioned envelope");
    let fields = envelope
        .as_object()
        .expect("envelope object")
        .keys()
        .map(String::as_str)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        fields,
        [
            "captured_at_us",
            "origin",
            "payload",
            "provider",
            "schema_version"
        ]
        .into_iter()
        .collect()
    );
    assert_eq!(envelope["schema_version"], 1);
    assert_eq!(envelope["provider"], "codex");
    assert!(
        envelope["captured_at_us"]
            .as_i64()
            .is_some_and(|time| time > 0)
    );
    assert_eq!(envelope["origin"]["kind"], "unresolved");
    assert!(
        envelope["origin"]["identity"]
            .as_str()
            .is_some_and(|identity| identity.starts_with("unresolved:"))
    );
    assert_eq!(envelope["origin"]["display_path"], "project");
    assert_eq!(envelope["payload"]["prompt"], "retain only in spool");
}

#[test]
fn capture_spools_unresolved_provenance_when_git_cannot_launch() {
    let data_dir = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("working directory");
    let empty_path = TempDir::new().expect("empty executable path");
    fs::write(data_dir.path().join("capture-enabled"), "true").expect("capture gate");
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "missing-git-session",
        "turn_id": "missing-git-turn",
        "cwd": cwd.path(),
        "prompt": "capture despite unavailable Git",
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .env("PATH", empty_path.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("capture process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("payload");
    let output = child.wait_with_output().expect("capture output");

    assert!(output.status.success(), "capture failed: {output:?}");
    let pending = fs::read_dir(data_dir.path().join("spool"))
        .expect("spool")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .find(|path| path.extension().is_some_and(|value| value == "pending"))
        .expect("pending envelope");
    let envelope: serde_json::Value =
        serde_json::from_slice(&fs::read(pending).expect("envelope")).expect("valid envelope");
    assert_eq!(envelope["origin"]["kind"], "unresolved");
    assert!(
        envelope["origin"]["identity"]
            .as_str()
            .is_some_and(|identity| identity.starts_with("unresolved:"))
    );
}

#[cfg(windows)]
#[test]
fn generated_hook_command_treats_shell_metacharacters_as_literal_path_text() {
    let directory = TempDir::new().expect("temporary directory");
    let data_dir = directory.path().join("state&literal");
    fs::create_dir_all(&data_dir).expect("data directory");
    fs::write(data_dir.join("capture-enabled"), "true").expect("capture gate");
    let command = akra_app::paths::hook_command(
        std::path::Path::new(env!("CARGO_BIN_EXE_akra-hookers")),
        &data_dir,
    )
    .expect("safe command");
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "safe-command-session",
        "turn_id": "safe-command-turn",
        "cwd": directory.path(),
        "prompt": "literal shell path",
    });
    let mut child = Command::new("cmd")
        .args(["/D", "/S", "/C", &command])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("generated hook command");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("payload");
    let output = child.wait_with_output().expect("hook output");

    assert!(output.status.success(), "hook command failed: {output:?}");
    assert_eq!(
        fs::read_dir(data_dir.join("spool"))
            .expect("spool")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "pending")
            })
            .count(),
        1
    );
}

#[test]
fn capture_rejects_oversized_valid_input_without_spooling() {
    let data_dir = TempDir::new().expect("data directory");
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "oversized-session",
        "turn_id": "oversized-turn",
        "cwd": "project",
        "prompt": "x".repeat(MAX_CAPTURE_INPUT_BYTES),
        "model": "test"
    })
    .to_string();
    assert!(payload.len() > MAX_CAPTURE_INPUT_BYTES);
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("capture starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.as_bytes())
        .expect("oversized input writes");

    let output = child.wait_with_output().expect("capture exits");
    assert_eq!(output.status.code(), Some(2), "capture output: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("exceeds"),
        "capture must report its finite input limit: {output:?}"
    );
    assert!(
        !data_dir.path().join("spool").exists(),
        "rejected input must not create a pending item"
    );
}

#[test]
fn capture_reports_a_full_spool_without_overwriting_pending_items() {
    let data_dir = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("working directory");
    let spool = data_dir.path().join("spool");
    fs::create_dir_all(&spool).expect("spool directory");
    for index in 0..1024 {
        fs::write(spool.join(format!("{index:04}.pending")), b"{}").expect("pending item");
    }

    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "full-session",
        "turn_id": "full-turn",
        "cwd": cwd.path(),
        "prompt": "must not overwrite",
        "model": "test"
    })
    .to_string();
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .stdin(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("capture starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.as_bytes())
        .expect("payload writes");
    let output = child.wait_with_output().expect("capture exits");

    assert_eq!(output.status.code(), Some(2), "capture output: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("pending spool queue is full"),
        "capture must report aggregate admission failure: {output:?}"
    );

    assert_eq!(
        fs::read_dir(spool)
            .expect("spool")
            .filter_map(Result::ok)
            .filter(|entry| entry
                .path()
                .extension()
                .is_some_and(|value| value == "pending"))
            .count(),
        1024
    );
}

#[test]
fn capture_surfaces_gate_read_errors_instead_of_dropping_the_prompt() {
    let data_dir = TempDir::new().expect("data directory");
    fs::create_dir_all(data_dir.path().join("capture-enabled")).expect("unreadable gate path");
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("capture starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"gate-error","turn_id":"turn","cwd":"project","prompt":"must not disappear","model":"test"}"#,
        )
        .expect("payload writes");

    let output = child.wait_with_output().expect("capture exits");
    assert_eq!(output.status.code(), Some(2), "capture output: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unable to read capture gate"),
        "capture must surface the gate error: {output:?}"
    );
    assert!(
        !data_dir.path().join("spool").exists(),
        "failed gate reads must not create a spool"
    );
}

#[test]
fn setup_writes_fast_synchronous_hook_to_default_and_active_codex_homes() {
    let user_home = TempDir::new().expect("user home");
    let codex_home = TempDir::new().expect("active Codex home");
    let data_dir = TempDir::new().expect("data directory");

    let output = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["setup", "--data-dir"])
        .arg(data_dir.path())
        .env("USERPROFILE", user_home.path())
        .env_remove("HOME")
        .env("CODEX_HOME", codex_home.path())
        .output()
        .expect("setup runs");

    assert!(output.status.success(), "setup failed: {output:?}");
    for manifest_path in [
        user_home.path().join(".codex").join("hooks.json"),
        codex_home.path().join("hooks.json"),
    ] {
        let hooks: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).expect("manifest"))
                .expect("valid manifest");
        let hook = &hooks["hooks"]["UserPromptSubmit"][0]["hooks"][0];
        assert_eq!(hook["type"], "command");
        assert!(
            hook.get("async").is_none(),
            "Codex skips async hooks, so capture must be synchronous"
        );
        assert_eq!(
            hook["timeout"], 1,
            "capture must have a tight bound while the daemon is unavailable"
        );
    }
}

#[test]
fn failed_cli_setup_restores_the_disabled_gate_and_valid_manifest() {
    let user_home = TempDir::new().expect("user home");
    let codex_home = TempDir::new().expect("active Codex home");
    let data_dir = TempDir::new().expect("data directory");
    let default_codex = user_home.path().join(".codex");
    fs::create_dir_all(&default_codex).expect("default Codex home");
    let valid_manifest = default_codex.join("hooks.json");
    fs::write(
        &valid_manifest,
        br#"{ "description": "preserve", "hooks": {} }"#,
    )
    .expect("valid manifest");
    let valid_before = fs::read(&valid_manifest).expect("valid bytes");
    fs::write(codex_home.path().join("hooks.json"), b"{").expect("malformed active manifest");

    let output = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["setup", "--data-dir"])
        .arg(data_dir.path())
        .env("USERPROFILE", user_home.path())
        .env_remove("HOME")
        .env("CODEX_HOME", codex_home.path())
        .output()
        .expect("setup runs");

    assert_eq!(output.status.code(), Some(2), "setup output: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unable to enable Codex capture"),
        "setup failure must be visible: {output:?}"
    );
    assert_eq!(
        fs::read_to_string(data_dir.path().join("capture-enabled")).expect("capture gate"),
        "false"
    );
    assert_eq!(
        fs::read(valid_manifest).expect("unchanged valid manifest"),
        valid_before
    );
}

#[test]
fn disabled_capture_exits_without_spooling_or_opening_sqlite() {
    let data_dir = TempDir::new().expect("data directory");
    fs::write(data_dir.path().join("capture-enabled"), "false").expect("capture gate");

    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("capture starts");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"session","turn_id":"turn","cwd":"project","prompt":"must not persist","model":"test"}"#,
        )
        .expect("payload writes");

    let output = child.wait_with_output().expect("capture exits");
    assert!(output.status.success(), "capture failed: {output:?}");
    assert!(output.stdout.is_empty(), "hook must remain silent");
    assert!(
        !data_dir.path().join("akra-hookers.sqlite").exists(),
        "disabled fast path must not open SQLite"
    );
    assert!(
        !data_dir.path().join("spool").exists(),
        "disabled fast path must not retain a new prompt"
    );
}

#[test]
fn failed_cli_disable_restores_the_gate_and_valid_hook_manifest() {
    let user_home = TempDir::new().expect("user home");
    let codex_home = TempDir::new().expect("active Codex home");
    let data_dir = TempDir::new().expect("data directory");
    let configure_environment = |command: &mut Command| {
        command
            .env("USERPROFILE", user_home.path())
            .env_remove("HOME")
            .env("CODEX_HOME", codex_home.path());
    };
    let mut setup = Command::new(env!("CARGO_BIN_EXE_akra-hookers"));
    setup.args(["setup", "--data-dir"]).arg(data_dir.path());
    configure_environment(&mut setup);
    let setup_output = setup.output().expect("setup runs");
    assert!(
        setup_output.status.success(),
        "setup failed: {setup_output:?}"
    );
    let valid_manifest = user_home.path().join(".codex").join("hooks.json");
    let valid_before = fs::read(&valid_manifest).expect("valid manifest");
    fs::write(codex_home.path().join("hooks.json"), b"{").expect("malformed active manifest");

    let mut disable = Command::new(env!("CARGO_BIN_EXE_akra-hookers"));
    disable.args(["disable", "--data-dir"]).arg(data_dir.path());
    configure_environment(&mut disable);
    let output = disable.output().expect("disable runs");

    assert_eq!(output.status.code(), Some(2), "disable output: {output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("unable to disable Codex capture"),
        "disable failure must be visible: {output:?}"
    );
    assert_eq!(
        fs::read_to_string(data_dir.path().join("capture-enabled")).expect("capture gate"),
        "true"
    );
    assert_eq!(
        fs::read(valid_manifest).expect("valid manifest after failure"),
        valid_before
    );
}

#[tokio::test]
async fn payload_accepted_before_disable_is_recovered_after_restart() {
    let data_dir = TempDir::new().expect("data directory");
    let payload = br#"{"hook_event_name":"UserPromptSubmit","session_id":"session","turn_id":"turn","cwd":"project","prompt":"accepted before disable","model":"test"}"#;
    Spool::open(&data_dir.path().join("spool"))
        .expect("spool")
        .enqueue(payload)
        .expect("accepted payload");

    let store = akra_store::ActivityStore::open(&data_dir.path().join("akra-hookers.sqlite"))
        .await
        .expect("store");
    store.migrate().await.expect("migration");
    store
        .set_provider_enabled("codex", false)
        .await
        .expect("provider disabled after acceptance");

    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["serve", "--port", "0", "--data-dir"])
        .arg(data_dir.path())
        .stdout(Stdio::piped())
        .spawn()
        .expect("runtime starts");
    let stdout = child.stdout.take().expect("runtime stdout");
    let (ready_sender, ready_receiver) = mpsc::channel();
    thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.expect("runtime output").starts_with("ready ") {
                ready_sender.send(()).expect("ready signal");
                return;
            }
        }
    });
    ready_receiver
        .recv_timeout(Duration::from_secs(5))
        .expect("runtime readiness");

    let recovered = store.activity_count().await.expect("activity count");
    let _ = child.kill();
    let _ = child.wait();
    assert_eq!(recovered, 1, "accepted payload must survive later disable");
}
