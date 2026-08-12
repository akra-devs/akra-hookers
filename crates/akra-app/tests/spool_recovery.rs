use std::{fs, path::Path};

use akra_app::{
    recovery::drain,
    spool::{CaptureEnvelope, Spool},
};
use akra_git::ProjectIdentity;
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

#[test]
fn spool_round_trips_a_pending_payload() {
    let directory = TempDir::new().expect("temp directory");
    let spool = Spool::open(directory.path()).expect("spool opens");
    spool
        .enqueue(br#"{"prompt":"recover me"}"#)
        .expect("payload spools");

    assert_eq!(
        spool.drain().expect("payload drains"),
        vec![br#"{"prompt":"recover me"}"#.to_vec()]
    );
}

#[test]
fn reading_pending_payload_does_not_acknowledge_it() {
    let directory = TempDir::new().expect("temp directory");
    let spool = Spool::open(directory.path()).expect("spool opens");
    spool
        .enqueue(br#"{"prompt":"retain me"}"#)
        .expect("payload spools");

    let payloads = spool.drain().expect("payload reads");
    assert_eq!(payloads, vec![br#"{"prompt":"retain me"}"#.to_vec()]);
    assert_eq!(
        fs::read_dir(directory.path())
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
        "a payload is only removed after durable storage acknowledges it"
    );
}

#[test]
fn envelope_v1_round_trips_exact_provider_payload_and_origin() {
    let directory = TempDir::new().expect("temp directory");
    let cwd = TempDir::new().expect("working directory");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin snapshot")
        .origin;
    let payload = codex_payload(cwd.path(), "session", "turn", "round trip");
    let envelope =
        CaptureEnvelope::new("codex", 123_456, origin, payload.clone()).expect("valid envelope");
    let spool = Spool::open(directory.path()).expect("spool opens");

    spool.enqueue_envelope(&envelope).expect("envelope spools");

    let pending = spool.pending().expect("pending items");
    assert_eq!(pending.len(), 1);
    let bytes = spool.read(&pending[0]).expect("pending payload");
    let value: serde_json::Value = serde_json::from_slice(&bytes).expect("envelope JSON");
    assert_eq!(value["schema_version"], 1);
    assert_eq!(value["provider"], "codex");
    assert_eq!(value["captured_at_us"], 123_456);
    assert_eq!(value["payload"], payload);
    assert_eq!(
        value.as_object().expect("envelope object").len(),
        5,
        "the wire envelope has only the reviewed fields"
    );
}

#[test]
fn envelope_v1_round_trips_optional_capture_source() {
    let cwd = TempDir::new().expect("working directory");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin snapshot")
        .origin;
    let payload = codex_payload(cwd.path(), "session", "turn", "source");
    let envelope = CaptureEnvelope::new_with_source(
        "codex",
        123_456,
        origin,
        payload,
        "windows-native",
        "app",
    )
    .expect("valid envelope");
    let bytes = serde_json::to_vec(&envelope).expect("envelope JSON");
    let decoded = CaptureEnvelope::decode(&bytes).expect("decoded envelope");

    assert_eq!(decoded.capture_source(), Some(("windows-native", "app")));
}

#[tokio::test]
async fn delayed_recovery_preserves_captured_origin_time_and_submitted_cwd() {
    let directory = TempDir::new().expect("test directory");
    let captured_cwd = directory.path().join("captured-cwd");
    fs::create_dir(&captured_cwd).expect("captured cwd");
    let snapshot =
        ProjectIdentity::capture_snapshot_from_cwd(&captured_cwd).expect("origin snapshot");
    let payload = codex_payload(&captured_cwd, "captured-session", "turn", "captured prompt");
    let envelope =
        CaptureEnvelope::new("codex", 42, snapshot.origin.clone(), payload).expect("envelope");
    let spool = Spool::open(&directory.path().join("spool")).expect("spool");
    spool.enqueue_envelope(&envelope).expect("envelope spools");
    fs::remove_dir(&captured_cwd).expect("captured cwd removed before recovery");
    let database = directory.path().join("akra.sqlite");
    let store = akra_store::ActivityStore::open(&database)
        .await
        .expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 1);

    let pool = test_pool(&database).await;
    let row: (
        String,
        String,
        String,
        Option<i64>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT activity_origins.identity, activity_origins.kind,
                    activity_origins.resolution_source, activity_events.captured_at_us,
                    activity_events.captured_at_provenance, activity_events.submitted_cwd
             FROM activity_events
             JOIN activity_origins ON activity_origins.id = activity_events.origin_id",
    )
    .fetch_one(&pool)
    .await
    .expect("capture metadata");
    assert_eq!(row.0, snapshot.origin.identity);
    assert_eq!(row.1, "directory");
    assert_eq!(row.2, "captured");
    assert_eq!(row.3, Some(42));
    assert_eq!(row.4.as_deref(), Some("captured"));
    assert_eq!(row.5.as_deref(), captured_cwd.to_str());
}

#[tokio::test]
async fn legacy_raw_payload_is_recovered_with_truthful_legacy_resolution() {
    let directory = TempDir::new().expect("test directory");
    let missing_cwd = directory.path().join("missing-at-drain");
    let spool = Spool::open(&directory.path().join("spool")).expect("spool");
    let payload = codex_payload(&missing_cwd, "legacy-session", "turn", "legacy prompt");
    spool
        .enqueue(&serde_json::to_vec(&payload).expect("legacy JSON"))
        .expect("legacy payload spools");
    let database = directory.path().join("akra.sqlite");
    let store = akra_store::ActivityStore::open(&database)
        .await
        .expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 1);

    let pool = test_pool(&database).await;
    let row: (String, String, Option<i64>, Option<String>) = sqlx::query_as(
        "SELECT activity_origins.kind, activity_origins.resolution_source,
                activity_events.captured_at_us, activity_events.submitted_cwd
         FROM activity_events
         JOIN activity_origins ON activity_origins.id = activity_events.origin_id",
    )
    .fetch_one(&pool)
    .await
    .expect("legacy metadata");
    assert_eq!(row.0, "unresolved");
    assert_eq!(row.1, "legacy_resolved");
    assert_eq!(row.2, None);
    assert_eq!(row.3.as_deref(), missing_cwd.to_str());
}

#[tokio::test]
async fn unknown_malformed_utf8_and_invalid_provider_payloads_remain_pending() {
    let directory = TempDir::new().expect("test directory");
    let spool_path = directory.path().join("spool");
    let spool = Spool::open(&spool_path).expect("spool");
    fs::write(
        spool_path.join("01-unknown.pending"),
        br#"{"schema_version":999,"provider":"codex","captured_at_us":1,"origin":{"identity":"x","kind":"unresolved","display_path":"x"},"payload":{}}"#,
    )
    .expect("unknown version");
    fs::write(spool_path.join("02-malformed.pending"), b"{").expect("malformed envelope");
    fs::write(spool_path.join("03-utf8.pending"), [0xff, 0xfe]).expect("invalid UTF-8");
    fs::write(
        spool_path.join("04-invalid-provider.pending"),
        br#"{"schema_version":1,"provider":"codex","captured_at_us":1,"origin":{"identity":"x","kind":"unresolved","display_path":"x"},"payload":{"prompt":"missing provider fields"}}"#,
    )
    .expect("invalid provider payload");
    fs::write(
        spool_path.join("05-negative-time.pending"),
        br#"{"schema_version":1,"provider":"codex","captured_at_us":-1,"origin":{"identity":"negative","kind":"unresolved","display_path":"x"},"payload":{"hook_event_name":"UserPromptSubmit","session_id":"negative","turn_id":"turn","cwd":"x","prompt":"negative time","model":"test"}}"#,
    )
    .expect("negative capture time");
    fs::write(
        spool_path.join("06-blank-origin.pending"),
        br#"{"schema_version":1,"provider":"codex","captured_at_us":1,"origin":{"identity":"","kind":"unresolved","display_path":"x"},"payload":{"hook_event_name":"UserPromptSubmit","session_id":"blank-origin","turn_id":"turn","cwd":"x","prompt":"blank origin","model":"test"}}"#,
    )
    .expect("blank origin identity");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 0);
    assert_eq!(store.activity_count().await.expect("activity count"), 0);
    assert_eq!(
        spool.pending().expect("pending").len(),
        6,
        "invalid bytes must remain available for retry"
    );
    assert!(!spool_path.join("quarantine").exists());
}

#[tokio::test]
async fn repaired_invalid_payload_retries_after_spool_reopen() {
    let directory = TempDir::new().expect("spool directory");
    let cwd = TempDir::new().expect("working directory");
    let spool_path = directory.path().join("spool");
    let pending_path = spool_path.join("repair.pending");
    let spool = Spool::open(&spool_path).expect("spool");
    fs::write(&pending_path, b"{").expect("invalid payload");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 0);
    assert!(pending_path.exists(), "invalid bytes remain pending");
    fs::write(
        &pending_path,
        serde_json::json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "repair-session",
            "turn_id": "repair-turn",
            "cwd": cwd.path(),
            "prompt": "repaired payload",
            "model": "test"
        })
        .to_string(),
    )
    .expect("repaired payload");

    assert_eq!(
        drain(&spool, &store).await,
        0,
        "deferred invalid path is not hot-looped in one process"
    );
    let reopened = Spool::open(&spool_path).expect("reopened spool");
    assert_eq!(drain(&reopened, &store).await, 1);
    assert!(!pending_path.exists());
}

fn codex_payload(cwd: &Path, session: &str, turn: &str, prompt: &str) -> serde_json::Value {
    json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": session,
        "turn_id": turn,
        "cwd": cwd,
        "prompt": prompt,
        "model": "test"
    })
}

async fn test_pool(path: &Path) -> sqlx::SqlitePool {
    SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(
            SqliteConnectOptions::new()
                .filename(path)
                .foreign_keys(true),
        )
        .await
        .expect("test pool")
}
