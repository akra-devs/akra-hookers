use std::{
    fs,
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::Duration,
};

use akra_app::spool::Spool;
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
            .count(),
        1,
        "validated hook input must produce one durable spool item"
    );
    assert!(
        !Spool::open(&data_dir.path().join("spool"))
            .expect("spool")
            .pending()
            .expect("pending")
            .is_empty(),
        "the retained payload must be readable without daemon startup"
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
