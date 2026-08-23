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

#[cfg(windows)]
fn wsl_test_executable() -> Option<std::path::PathBuf> {
    let executable = std::path::PathBuf::from(std::env::var_os("SystemRoot")?)
        .join("System32")
        .join("wsl.exe");
    executable.is_file().then_some(executable)
}

#[cfg(windows)]
fn default_wsl_distro(wsl: &std::path::Path) -> Option<String> {
    let output = Command::new(wsl)
        .args(["--", "printenv", "WSL_DISTRO_NAME"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let distro = std::str::from_utf8(&output.stdout).ok()?.trim().to_owned();
    if distro.is_empty() || distro.to_ascii_lowercase().starts_with("docker-desktop") {
        return None;
    }
    let git = Command::new(wsl)
        .args(["-d", &distro, "--", "git", "--version"])
        .output()
        .ok()?;
    git.status.success().then_some(distro)
}

#[cfg(windows)]
fn run_wsl(wsl: &std::path::Path, distro: &str, arguments: &[&str]) -> std::process::Output {
    Command::new(wsl)
        .args(["-d", distro, "--"])
        .args(arguments)
        .output()
        .expect("WSL command launches")
}

#[cfg(windows)]
fn assert_wsl_success(output: &std::process::Output, operation: &str) {
    assert!(
        output.status.success(),
        "{operation} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
struct WslTestDirectory {
    wsl: std::path::PathBuf,
    distro: String,
    root: String,
}

#[cfg(windows)]
impl WslTestDirectory {
    fn new(wsl: &std::path::Path, distro: &str) -> Self {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("test clock")
            .as_nanos();
        let root = format!(
            "/tmp/akra-hookers-wsl-origin-{}-{unique}",
            std::process::id()
        );
        assert_wsl_success(
            &run_wsl(wsl, distro, &["mkdir", "-p", "--", &root]),
            "create WSL test root",
        );
        Self {
            wsl: wsl.to_path_buf(),
            distro: distro.to_owned(),
            root,
        }
    }
}

#[cfg(windows)]
impl Drop for WslTestDirectory {
    fn drop(&mut self) {
        let prefix = "/tmp/akra-hookers-wsl-origin-";
        let suffix = self.root.strip_prefix(prefix);
        if suffix.is_some_and(|suffix| {
            !suffix.is_empty()
                && suffix
                    .chars()
                    .all(|character| character.is_ascii_digit() || character == '-')
        }) {
            let _ = run_wsl(&self.wsl, &self.distro, &["rm", "-rf", "--", &self.root]);
        }
    }
}

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
fn remote_capture_stops_after_durable_outbox_enqueue() {
    let data_dir = TempDir::new().expect("data directory");
    let manager = akra_app::collector::CollectorManager::open(data_dir.path()).expect("collector");
    manager
        .configure(akra_app::collector::CollectorConfigInput {
            endpoint: "https://collector.invalid".to_owned(),
            token: Some("test-token".to_owned()),
        })
        .expect("remote destination");

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
        .as_mut()
        .expect("stdin")
        .write_all(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"remote-session","turn_id":"remote-turn","cwd":"project","prompt":"queue without relay","model":"test"}"#,
        )
        .expect("payload writes");

    let output = child.wait_with_output().expect("capture exits");
    assert!(output.status.success(), "capture failed: {output:?}");
    assert!(
        output.stdout.is_empty(),
        "prompt hook output must stay empty"
    );
    assert!(
        output.stderr.is_empty(),
        "capture must not attempt or report remote delivery: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let outbox = Spool::open(&data_dir.path().join("remote-outbox")).expect("outbox");
    assert_eq!(outbox.pending().expect("pending outbox").len(), 1);
    assert!(
        fs::read_dir(data_dir.path().join("remote-outbox-retry"))
            .expect("retry directory")
            .next()
            .is_none(),
        "hook capture must not create relay retry state"
    );
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

#[cfg(windows)]
#[test]
fn wsl_capture_uses_distro_git_identity_without_changing_safe_directory() {
    let Some(wsl) = wsl_test_executable() else {
        return;
    };
    let Some(distro) = default_wsl_distro(&wsl) else {
        return;
    };
    let wsl_directory = WslTestDirectory::new(&wsl, &distro);
    let primary = format!("{}/primary", wsl_directory.root);
    let linked = format!("{}/linked", wsl_directory.root);
    let primary_nested = format!("{primary}/nested");
    let linked_nested = format!("{linked}/nested");

    assert_wsl_success(
        &run_wsl(&wsl, &distro, &["mkdir", "-p", "--", &primary]),
        "create primary repo",
    );
    for arguments in [
        vec!["git", "-C", &primary, "init"],
        vec![
            "git",
            "-C",
            &primary,
            "config",
            "user.email",
            "tests@example.invalid",
        ],
        vec![
            "git",
            "-C",
            &primary,
            "config",
            "user.name",
            "akra-hookers tests",
        ],
        vec![
            "git",
            "-C",
            &primary,
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
        vec![
            "git", "-C", &primary, "worktree", "add", "-b", "linked", &linked,
        ],
    ] {
        assert_wsl_success(&run_wsl(&wsl, &distro, &arguments), "prepare WSL repo");
    }
    assert_wsl_success(
        &run_wsl(
            &wsl,
            &distro,
            &["mkdir", "-p", "--", &primary_nested, &linked_nested],
        ),
        "create nested worktree directories",
    );

    let safe_directory_before = run_wsl(
        &wsl,
        &distro,
        &["git", "config", "--global", "--get-all", "safe.directory"],
    );
    let data_dir = TempDir::new().expect("data directory");
    fs::write(data_dir.path().join("capture-enabled"), "true").expect("capture gate");
    for (session_id, cwd) in [
        ("wsl-primary-session", primary_nested.as_str()),
        ("wsl-linked-session", linked_nested.as_str()),
    ] {
        let payload = serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session_id,
            "turn_id": "turn",
            "cwd": cwd,
            "prompt": "resolve through WSL Git",
            "model": "test",
        });
        let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
            .args(["capture", "--data-dir"])
            .arg(data_dir.path())
            .args(["--wsl-distro", &distro])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("WSL capture process");
        child
            .stdin
            .take()
            .expect("stdin")
            .write_all(payload.to_string().as_bytes())
            .expect("WSL payload");
        let output = child.wait_with_output().expect("WSL capture output");
        assert!(output.status.success(), "WSL capture failed: {output:?}");
    }

    let spool = Spool::open(&data_dir.path().join("spool")).expect("spool");
    let mut envelopes = std::collections::BTreeMap::new();
    for item in spool.pending().expect("pending envelopes") {
        let bytes = spool.read(&item).expect("envelope bytes");
        let envelope: serde_json::Value = serde_json::from_slice(&bytes).expect("valid envelope");
        let session_id = envelope["payload"]["session_id"]
            .as_str()
            .expect("session id")
            .to_owned();
        envelopes.insert(session_id, envelope);
    }
    let primary_envelope = &envelopes["wsl-primary-session"];
    let linked_envelope = &envelopes["wsl-linked-session"];
    assert_eq!(primary_envelope["origin"]["kind"], "git");
    assert_eq!(linked_envelope["origin"]["kind"], "git");
    assert_eq!(
        primary_envelope["origin"]["identity"],
        linked_envelope["origin"]["identity"]
    );
    assert_eq!(
        primary_envelope["origin"]["display_path"],
        format!("wsl://{distro}{primary}")
    );
    assert_eq!(
        linked_envelope["origin"]["display_path"],
        format!("wsl://{distro}{linked}")
    );

    let safe_directory_after = run_wsl(
        &wsl,
        &distro,
        &["git", "config", "--global", "--get-all", "safe.directory"],
    );
    assert_eq!(
        safe_directory_after.status.code(),
        safe_directory_before.status.code()
    );
    assert_eq!(safe_directory_after.stdout, safe_directory_before.stdout);
    assert_eq!(safe_directory_after.stderr, safe_directory_before.stderr);
}

#[cfg(windows)]
#[test]
fn wsl_mounted_windows_repo_keeps_the_native_windows_identity() {
    let repository = TempDir::new().expect("repository");
    for arguments in [
        vec!["init"],
        vec!["config", "user.email", "tests@example.invalid"],
        vec!["config", "user.name", "akra-hookers tests"],
        vec!["commit", "--allow-empty", "-m", "initial"],
    ] {
        let output = Command::new("git")
            .arg("-C")
            .arg(repository.path())
            .args(arguments)
            .output()
            .expect("Windows Git command");
        assert!(output.status.success(), "Git setup failed: {output:?}");
    }
    let expected = akra_git::ProjectIdentity::capture_snapshot_from_cwd(repository.path())
        .expect("Windows project identity")
        .origin;
    let repository_text = repository.path().to_string_lossy();
    let repository_text = repository_text
        .strip_prefix(r"\\?\")
        .unwrap_or(&repository_text);
    let bytes = repository_text.as_bytes();
    assert_eq!(bytes.get(1), Some(&b':'), "drive-qualified test path");
    let drive = char::from(bytes[0]).to_ascii_lowercase();
    let wsl_cwd = format!("/mnt/{drive}/{}", repository_text[3..].replace('\\', "/"));

    let data_dir = TempDir::new().expect("data directory");
    fs::write(data_dir.path().join("capture-enabled"), "true").expect("capture gate");
    let payload = serde_json::json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "wsl-mounted-windows-session",
        "turn_id": "turn",
        "cwd": wsl_cwd,
        "prompt": "share Windows identity",
        "model": "test",
    });
    let mut child = Command::new(env!("CARGO_BIN_EXE_akra-hookers"))
        .args(["capture", "--data-dir"])
        .arg(data_dir.path())
        .args(["--wsl-distro", "Ubuntu"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("mounted WSL capture process");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload.to_string().as_bytes())
        .expect("mounted WSL payload");
    let output = child
        .wait_with_output()
        .expect("mounted WSL capture output");
    assert!(output.status.success(), "WSL capture failed: {output:?}");

    let spool = Spool::open(&data_dir.path().join("spool")).expect("spool");
    let pending = spool.pending().expect("pending envelopes");
    assert_eq!(pending.len(), 1);
    let envelope: serde_json::Value =
        serde_json::from_slice(&spool.read(&pending[0]).expect("envelope bytes"))
            .expect("valid envelope");
    assert_eq!(envelope["origin"]["kind"], "git");
    assert_eq!(envelope["origin"]["identity"], expected.identity);
    assert_eq!(
        envelope["origin"]["display_path"],
        expected.display_path.to_string_lossy().as_ref()
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
    assert_eq!(output.status.code(), Some(1), "capture output: {output:?}");
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

    assert_eq!(output.status.code(), Some(1), "capture output: {output:?}");
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
    assert_eq!(output.status.code(), Some(1), "capture output: {output:?}");
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
        .env("AKRA_HOOKERS_SKIP_WSL", "1")
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
            hook["timeout"], 5,
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
        .env("AKRA_HOOKERS_SKIP_WSL", "1")
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
            .env("CODEX_HOME", codex_home.path())
            .env("AKRA_HOOKERS_SKIP_WSL", "1");
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
