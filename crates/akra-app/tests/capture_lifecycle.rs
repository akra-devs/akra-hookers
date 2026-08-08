use std::{
    fs,
    io::Write,
    process::{Command, Stdio},
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
