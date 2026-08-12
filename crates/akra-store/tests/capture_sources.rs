use akra_core::ingress::IngressEvent;
use akra_store::{ActivityStore, CaptureClientObservation, RecordActivity};
use tempfile::TempDir;

#[tokio::test]
async fn records_latest_capture_evidence_per_target_and_client() {
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");
    let cwd = TempDir::new().expect("working directory");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin")
        .origin;

    for (turn, client, captured_at_us) in
        [("one", "app", 10), ("two", "app", 20), ("three", "cli", 15)]
    {
        let event = IngressEvent::try_new(
            "codex",
            "session",
            turn,
            cwd.path().to_string_lossy(),
            turn,
            None,
        )
        .expect("event");
        store
            .record(RecordActivity::captured_from(
                event,
                origin.clone(),
                captured_at_us,
                "windows-native",
                client,
            ))
            .await
            .expect("record");
    }

    assert_eq!(
        store
            .capture_client_observations()
            .await
            .expect("observations"),
        vec![
            CaptureClientObservation {
                target_id: "windows-native".to_owned(),
                client: "app".to_owned(),
                last_captured_at_us: 20,
            },
            CaptureClientObservation {
                target_id: "windows-native".to_owned(),
                client: "cli".to_owned(),
                last_captured_at_us: 15,
            },
        ]
    );
}
