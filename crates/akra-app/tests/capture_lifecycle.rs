use std::{
    io::Write,
    process::{Command, Stdio},
};

use akra_app::spool::Spool;
use tempfile::TempDir;

#[tokio::test]
async fn disabled_provider_does_not_spool_hook_capture() {
    let data_dir = TempDir::new().expect("data directory");
    let store = akra_store::ActivityStore::open(&data_dir.path().join("akra-hookers.sqlite"))
        .await
        .expect("store");
    store.migrate().await.expect("migration");
    store
        .set_provider_enabled("codex", false)
        .await
        .expect("provider disabled");

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
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"session","turn_id":"turn","cwd":"project","prompt":"do not retain","model":"test"}"#,
        )
        .expect("payload writes");

    let output = child.wait_with_output().expect("capture exits");
    assert!(output.status.success(), "capture failed: {output:?}");
    assert!(
        Spool::open(&data_dir.path().join("spool"))
            .expect("spool")
            .pending()
            .expect("pending")
            .is_empty(),
        "disabled providers must not leave future prompts on disk"
    );
}
