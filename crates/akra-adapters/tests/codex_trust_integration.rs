use std::{
    fs,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use akra_adapters::codex::CodexHookLifecycle;
use serde_json::{Value, json};
use tempfile::TempDir;

const RUN_INTEGRATION_ENV: &str = "AKRA_RUN_CODEX_TRUST_INTEGRATION";
const VERIFY_HOME_ENV: &str = "AKRA_VERIFY_CODEX_HOME";
const TEST_COMMAND: &str = "akra-hookers capture";

#[test]
fn installed_codex_reports_generated_hook_state_as_trusted() {
    if std::env::var_os(RUN_INTEGRATION_ENV).as_deref() != Some(std::ffi::OsStr::new("1")) {
        eprintln!("skipping installed Codex integration; set {RUN_INTEGRATION_ENV}=1 to run");
        return;
    }

    let home = TempDir::new().expect("temporary home");
    let codex_home = home.path().join(".codex");
    CodexHookLifecycle::new(home.path())
        .enable(TEST_COMMAND)
        .expect("enable hook with trusted state");

    let response = list_hooks(&codex_home, home.path())
        .unwrap_or_else(|error| panic!("query installed Codex hooks: {error}"));
    assert_trusted_hook(&response, TEST_COMMAND);
}

#[test]
fn installed_codex_reports_existing_akra_hook_as_trusted() {
    let Some(codex_home) = std::env::var_os(VERIFY_HOME_ENV).map(std::path::PathBuf::from) else {
        eprintln!("skipping live Codex verification; set {VERIFY_HOME_ENV} to run");
        return;
    };
    let expected_command = managed_hook_command(&codex_home)
        .unwrap_or_else(|error| panic!("read installed Akra hook: {error}"));
    let cwd = codex_home.parent().unwrap_or(&codex_home);
    let response = list_hooks(&codex_home, cwd)
        .unwrap_or_else(|error| panic!("query installed Codex hooks: {error}"));

    assert_trusted_hook(&response, &expected_command);
}

fn list_hooks(codex_home: &Path, cwd: &Path) -> Result<Value, String> {
    let mut child = spawn_codex_app_server(codex_home)
        .map_err(|error| format!("start installed Codex app-server: {error}"))?;
    let stdout = child.stdout.take().expect("Codex stdout");
    let mut stdin = child.stdin.take().expect("Codex stdin");
    let (line_sender, line_receiver) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let line = match line {
                Ok(line) => line,
                Err(error) => {
                    let _ = line_sender.send(Err(error.to_string()));
                    break;
                }
            };
            if line_sender.send(Ok(line)).is_err() {
                break;
            }
        }
    });

    let result = query_hooks(&mut stdin, &line_receiver, cwd);

    drop(stdin);
    let _ = child.kill();
    let _ = child.wait();
    reader
        .join()
        .map_err(|_| "Codex stdout reader panicked".to_owned())?;

    result
}

fn assert_trusted_hook(response: &Value, expected_command: &str) {
    let hooks = response["result"]["data"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| entry["hooks"].as_array())
        .flatten()
        .filter(|hook| hook["command"] == expected_command)
        .collect::<Vec<_>>();
    let hooks = if hooks.is_empty() {
        panic!("Akra hook missing from hooks/list response: {response}");
    } else {
        hooks
    };
    assert_eq!(
        hooks.len(),
        2,
        "UserPromptSubmit and SubagentStart must both be installed: {response}"
    );
    for hook in hooks {
        assert_eq!(hook["trustStatus"], "trusted", "response: {response}");
        assert_eq!(hook["enabled"], true, "response: {response}");
    }
}

fn managed_hook_command(codex_home: &Path) -> Result<String, String> {
    let manifest_path = codex_home.join("hooks.json");
    let manifest: Value = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("parse {}: {error}", manifest_path.display()))?;
    let hook = manifest["hooks"]["UserPromptSubmit"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|group| group["hooks"].as_array())
        .flatten()
        .find(|hook| hook["akraHookersManaged"] == true)
        .ok_or_else(|| format!("managed Akra hook missing from {}", manifest_path.display()))?;

    #[cfg(windows)]
    let command = hook["commandWindows"]
        .as_str()
        .or_else(|| hook["command"].as_str());
    #[cfg(not(windows))]
    let command = hook["command"].as_str();

    command
        .map(ToOwned::to_owned)
        .ok_or_else(|| "managed Akra hook command is missing".to_owned())
}

fn spawn_codex_app_server(codex_home: &std::path::Path) -> std::io::Result<Child> {
    #[cfg(windows)]
    let mut command = {
        let mut command = Command::new("cmd.exe");
        command.args(["/d", "/s", "/c", "codex app-server --stdio"]);
        command
    };

    #[cfg(not(windows))]
    let mut command = {
        let mut command = Command::new("codex");
        command.args(["app-server", "--stdio"]);
        command
    };

    command
        .env("CODEX_HOME", codex_home)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
}

fn query_hooks(
    stdin: &mut impl Write,
    lines: &mpsc::Receiver<Result<String, String>>,
    cwd: &std::path::Path,
) -> Result<Value, String> {
    send_message(
        stdin,
        json!({
            "id": 1,
            "method": "initialize",
            "params": {
                "clientInfo": {
                    "name": "akra-hookers-verifier",
                    "title": "Akra Hookers verifier",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true,
                    "requestAttestation": false
                }
            }
        }),
    )?;

    let deadline = Instant::now() + Duration::from_secs(15);
    let mut hooks_requested = false;
    loop {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .ok_or_else(|| "timed out waiting for Codex app-server".to_owned())?;
        let line = lines
            .recv_timeout(remaining)
            .map_err(|error| format!("Codex app-server closed or timed out: {error}"))?
            .map_err(|error| format!("read Codex app-server output: {error}"))?;
        let message: Value = serde_json::from_str(&line)
            .map_err(|error| format!("invalid Codex app-server JSON `{line}`: {error}"))?;

        if message["id"] == 1 && !hooks_requested {
            send_message(stdin, json!({ "method": "initialized" }))?;
            send_message(
                stdin,
                json!({
                    "id": 2,
                    "method": "hooks/list",
                    "params": { "cwds": [cwd] }
                }),
            )?;
            hooks_requested = true;
        } else if message["id"] == 2 {
            if message.get("error").is_some() {
                return Err(format!("hooks/list failed: {message}"));
            }
            return Ok(message);
        }
    }
}

fn send_message(stdin: &mut impl Write, message: Value) -> Result<(), String> {
    serde_json::to_writer(&mut *stdin, &message)
        .map_err(|error| format!("serialize app-server message: {error}"))?;
    stdin
        .write_all(b"\n")
        .and_then(|()| stdin.flush())
        .map_err(|error| format!("write app-server message: {error}"))
}
