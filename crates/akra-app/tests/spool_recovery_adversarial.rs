use std::{fs, path::Path};

use akra_app::{
    recovery::drain,
    spool::{CaptureEnvelope, MAX_PENDING_ITEM_BYTES, RECOVERY_BATCH_SIZE, Spool},
};
use akra_git::ProjectIdentity;
use serde_json::json;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};
use tempfile::TempDir;

#[tokio::test]
async fn database_failure_retains_the_complete_envelope() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool = Spool::open(&directory.path().join("spool")).expect("spool");
    let envelope = envelope(cwd.path(), "session", "turn", "retain after failure", 10);
    spool.enqueue_envelope(&envelope).expect("envelope spools");
    let unmigrated_store = akra_store::ActivityStore::in_memory().await.expect("store");

    assert_eq!(drain(&spool, &unmigrated_store).await, 0);
    assert_eq!(spool.pending().expect("pending").len(), 1);
}

#[tokio::test]
async fn duplicate_envelope_replay_keeps_one_event_and_acknowledges_both_files() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool = Spool::open(&directory.path().join("spool")).expect("spool");
    let envelope = envelope(cwd.path(), "same-session", "same-turn", "deduplicated", 20);
    spool.enqueue_envelope(&envelope).expect("first envelope");
    spool
        .enqueue_envelope(&envelope)
        .expect("replayed envelope");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 2);
    assert_eq!(store.activity_count().await.expect("activity count"), 1);
    assert!(spool.pending().expect("pending").is_empty());
}

#[tokio::test]
async fn reverse_filename_recovery_preserves_each_captured_time_and_origin() {
    let directory = TempDir::new().expect("test directory");
    let first_cwd = directory.path().join("first");
    let second_cwd = directory.path().join("second");
    fs::create_dir(&first_cwd).expect("first cwd");
    fs::create_dir(&second_cwd).expect("second cwd");
    let first = envelope(&first_cwd, "session", "first", "first prompt", 100);
    let second = envelope(&second_cwd, "session", "second", "second prompt", 200);
    let first_identity = first.origin().identity.clone();
    let second_identity = second.origin().identity.clone();
    let spool_path = directory.path().join("spool");
    fs::create_dir(&spool_path).expect("spool directory");
    fs::write(
        spool_path.join("99-older.pending"),
        serde_json::to_vec(&first).expect("first JSON"),
    )
    .expect("first pending");
    fs::write(
        spool_path.join("01-newer.pending"),
        serde_json::to_vec(&second).expect("second JSON"),
    )
    .expect("second pending");
    fs::remove_dir(&first_cwd).expect("first cwd removed");
    fs::remove_dir(&second_cwd).expect("second cwd removed");
    let database = directory.path().join("akra.sqlite");
    let store = akra_store::ActivityStore::open(&database)
        .await
        .expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(
        drain(&Spool::open(&spool_path).expect("spool"), &store).await,
        2
    );

    let pool = test_pool(&database).await;
    let rows: Vec<(String, Option<i64>, String)> = sqlx::query_as(
        "SELECT activity_events.prompt, activity_events.captured_at_us,
                activity_origins.identity
         FROM activity_events
         JOIN activity_origins ON activity_origins.id = activity_events.origin_id
         ORDER BY activity_events.prompt",
    )
    .fetch_all(&pool)
    .await
    .expect("capture rows");
    assert_eq!(
        rows,
        vec![
            ("first prompt".to_owned(), Some(100), first_identity),
            ("second prompt".to_owned(), Some(200), second_identity),
        ]
    );
}

#[tokio::test]
async fn one_pass_processes_only_a_bounded_batch() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool = Spool::open(&directory.path().join("spool")).expect("spool");
    for index in 0..=RECOVERY_BATCH_SIZE {
        spool
            .enqueue_envelope(&envelope(
                cwd.path(),
                "batch-session",
                &format!("turn-{index}"),
                &format!("prompt-{index}"),
                i64::try_from(index).expect("capture time"),
            ))
            .expect("envelope spools");
    }
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, RECOVERY_BATCH_SIZE);
    assert_eq!(
        store.activity_count().await.expect("activity count"),
        i64::try_from(RECOVERY_BATCH_SIZE).expect("batch count")
    );
    assert_eq!(pending_file_count(&directory.path().join("spool")), 1);
}

#[tokio::test]
async fn invalid_oversized_and_non_regular_items_are_deferred_before_valid_recovery() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool_path = directory.path().join("spool");
    let spool = Spool::open(&spool_path).expect("spool");
    let malformed = b"{preserve these malformed bytes";
    fs::write(spool_path.join("01-malformed.pending"), malformed).expect("malformed item");
    fs::write(
        spool_path.join("02-oversized.pending"),
        vec![b'o'; MAX_PENDING_ITEM_BYTES + 1],
    )
    .expect("oversized item");
    fs::create_dir(spool_path.join("03-directory.pending")).expect("non-regular item");
    fs::write(
        spool_path.join("04-valid.pending"),
        serde_json::to_vec(&envelope(cwd.path(), "valid", "turn", "later valid", 1))
            .expect("valid JSON"),
    )
    .expect("valid item");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 1);
    assert_eq!(store.activity_count().await.expect("activity count"), 1);
    assert_eq!(pending_file_count(&spool_path), 3);
    assert_eq!(
        fs::read(spool_path.join("01-malformed.pending")).expect("malformed bytes"),
        malformed
    );
    assert!(!spool_path.join("quarantine").exists());
}

#[tokio::test]
async fn unrelated_entries_cannot_hide_a_valid_pending_item() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool_path = directory.path().join("spool");
    let spool = Spool::open(&spool_path).expect("spool");
    for index in 0..300 {
        fs::write(spool_path.join(format!("{index:03}.tmp")), b"stale").expect("unrelated entry");
    }
    fs::write(
        spool_path.join("zzz.pending"),
        serde_json::to_vec(&envelope(cwd.path(), "visible", "turn", "visible", 1))
            .expect("envelope JSON"),
    )
    .expect("pending payload");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 1);
    assert!(!spool_path.join("zzz.pending").exists());
}

#[tokio::test]
async fn persistent_failures_do_not_starve_later_pending_items() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool_path = directory.path().join("spool");
    let spool = Spool::open(&spool_path).expect("spool");
    for index in 0..=RECOVERY_BATCH_SIZE {
        fs::write(
            spool_path.join(format!("{index:03}.pending")),
            serde_json::to_vec(&envelope(
                cwd.path(),
                "fair-session",
                &format!("turn-{index}"),
                "fair recovery",
                i64::try_from(index).expect("capture time"),
            ))
            .expect("envelope JSON"),
        )
        .expect("pending payload");
    }
    let store = akra_store::ActivityStore::in_memory().await.expect("store");

    assert_eq!(drain(&spool, &store).await, 0);
    store.migrate().await.expect("migration");
    assert_eq!(drain(&spool, &store).await, RECOVERY_BATCH_SIZE);
    assert!(
        !spool_path
            .join(format!("{RECOVERY_BATCH_SIZE:03}.pending"))
            .exists(),
        "the item just beyond the first failed batch must make progress"
    );
}

#[tokio::test]
async fn one_long_lived_spool_defers_a_full_invalid_batch_before_valid_recovery() {
    let directory = TempDir::new().expect("test directory");
    let cwd = TempDir::new().expect("working directory");
    let spool_path = directory.path().join("spool");
    let spool = Spool::open(&spool_path).expect("spool");
    for index in 0..RECOVERY_BATCH_SIZE {
        fs::write(spool_path.join(format!("{index:03}.pending")), b"{")
            .expect("invalid pending payload");
    }
    let valid_path = spool_path.join(format!("{RECOVERY_BATCH_SIZE:03}.pending"));
    fs::write(
        &valid_path,
        serde_json::to_vec(&envelope(
            cwd.path(),
            "long-lived",
            "valid-turn",
            "recover after invalid batch",
            1,
        ))
        .expect("valid envelope"),
    )
    .expect("valid pending payload");
    let store = akra_store::ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 0);
    assert_eq!(drain(&spool, &store).await, 1);
    assert!(!valid_path.exists());
}

#[test]
fn enqueue_rejects_aggregate_item_and_byte_overflow() {
    let item_directory = TempDir::new().expect("item directory");
    let item_spool = Spool::open(item_directory.path()).expect("item spool");
    for index in 0..1024 {
        fs::write(
            item_directory.path().join(format!("{index:04}.pending")),
            b"{}",
        )
        .expect("pending item");
    }
    assert!(item_spool.enqueue(b"{}").is_err());
    assert_eq!(pending_file_count(item_directory.path()), 1024);

    let byte_directory = TempDir::new().expect("byte directory");
    let byte_spool = Spool::open(byte_directory.path()).expect("byte spool");
    for index in 0..32 {
        let file = fs::File::create(byte_directory.path().join(format!("{index:02}.pending")))
            .expect("pending file");
        file.set_len((2 * 1024 * 1024) as u64)
            .expect("pending file length");
    }
    assert!(byte_spool.enqueue(b"{}").is_err());
    assert_eq!(pending_file_count(byte_directory.path()), 32);
}

#[test]
fn recovery_rejects_an_unbounded_directory_scan() {
    let directory = TempDir::new().expect("spool directory");
    let spool = Spool::open(directory.path()).expect("spool");
    for index in 0..4097 {
        fs::write(directory.path().join(format!("{index:04}.tmp")), b"stale")
            .expect("unrelated entry");
    }

    assert!(matches!(
        spool.pending(),
        Err(akra_app::spool::SpoolError::QueueInspectionLimit { .. })
    ));
}

fn pending_file_count(path: &Path) -> usize {
    fs::read_dir(path)
        .expect("spool directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().extension().is_some_and(|ext| ext == "pending"))
        .count()
}

fn envelope(
    cwd: &Path,
    session: &str,
    turn: &str,
    prompt: &str,
    captured_at_us: i64,
) -> CaptureEnvelope {
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin snapshot")
        .origin;
    CaptureEnvelope::new(
        "codex",
        captured_at_us,
        origin,
        json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": session,
            "turn_id": turn,
            "cwd": cwd,
            "prompt": prompt,
            "model": "test"
        }),
    )
    .expect("envelope")
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
