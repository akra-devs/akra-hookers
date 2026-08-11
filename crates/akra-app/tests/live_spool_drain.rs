use akra_app::{recovery::drain, spool::Spool};
use tempfile::TempDir;

#[tokio::test]
async fn drain_persists_a_new_codex_payload_without_runtime_restart() {
    let directory = TempDir::new().expect("spool directory");
    let spool = Spool::open(directory.path()).expect("spool opens");
    let store = akra_store::ActivityStore::in_memory()
        .await
        .expect("store opens");
    store.migrate().await.expect("store migrates");
    spool
        .enqueue(
            br#"{"hook_event_name":"UserPromptSubmit","session_id":"live-session","turn_id":"live-turn","cwd":"C:\\dev\\akra-hookers","prompt":"live spool recovery","model":"test"}"#,
        )
        .expect("payload spools");

    assert_eq!(drain(&spool, &store).await, 1);
    assert_eq!(
        store
            .activities()
            .await
            .expect("activities")
            .into_iter()
            .map(|activity| activity.prompt)
            .collect::<Vec<_>>(),
        vec!["live spool recovery"]
    );
    assert!(
        spool.pending().expect("pending spool").is_empty(),
        "durably stored payload must be acknowledged"
    );
}
