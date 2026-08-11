use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{ActivityStore, RecordActivity};
use tempfile::TempDir;

#[path = "support/project_context.rs"]
mod support;
use support::*;

async fn seed_production_legacy_writer_fixture(pool: &sqlx::SqlitePool, project_identity: &str) {
    sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
        .execute(pool)
        .await
        .expect("production v1 schema");
    sqlx::query("INSERT INTO projects (id, identity, display_path) VALUES (10, ?, ?)")
        .bind(project_identity)
        .bind(project_identity)
        .execute(pool)
        .await
        .expect("legacy project");
    sqlx::query(
        "INSERT INTO activity_events
         (id, provider, provider_session_id, provider_turn_id, project_identity, prompt, created_at)
         VALUES (101, 'codex', 'legacy-session', 'legacy-turn', ?, 'original prompt',
                 '2025-01-01 00:00:00')",
    )
    .bind(project_identity)
    .execute(pool)
    .await
    .expect("legacy writer activity");
    sqlx::query(
        "INSERT INTO canvas_nodes (id, activity_event_id, position_x, position_y)
         VALUES (201, 101, 12.5, -7.25)",
    )
    .execute(pool)
    .await
    .expect("legacy canvas fact");
}

#[tokio::test]
async fn migration_backfills_legacy_writer_dedupe_for_idempotent_replay() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("legacy-writer-replay.sqlite");
    let cwd = directory.path().to_string_lossy().into_owned();
    let pool = fixture_pool(&path, false).await;
    seed_production_legacy_writer_fixture(&pool, &cwd).await;
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM ingest_dedupes").await,
        0
    );
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    store.migrate().await.expect("v2 migration succeeds");
    drop(store);

    let pool = open_pool(&path).await;
    let activity_facts = rows(
        &pool,
        "SELECT id, provider, provider_session_id, provider_turn_id, project_identity, prompt,
                created_at, origin_id, first_recorded_at_us, first_recorded_at_provenance,
                global_sequence
         FROM activity_events ORDER BY id",
        11,
    )
    .await;
    let canvas_facts = rows(
        &pool,
        "SELECT id, activity_event_id, position_x, position_y FROM canvas_nodes ORDER BY id",
        4,
    )
    .await;
    drop(pool);

    let event = IngressEvent::try_new(
        "codex",
        "legacy-session",
        "legacy-turn",
        &cwd,
        "replayed payload must not overwrite facts",
        None,
    )
    .expect("valid replay event");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(directory.path())
        .expect("fixture origin")
        .origin;
    let store = ActivityStore::open(&path).await.expect("store reopens");
    let replayed_id = store
        .record(RecordActivity::legacy_resolved(event, origin))
        .await
        .expect("legacy replay is idempotent");
    assert_eq!(replayed_id, 101, "replay returns the original activity");
    drop(store);

    let pool = open_pool(&path).await;
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM activity_events AS activity
             LEFT JOIN ingest_dedupes AS dedupe
               ON dedupe.provider = activity.provider
              AND dedupe.provider_session_id = activity.provider_session_id
              AND dedupe.provider_turn_id = activity.provider_turn_id
              AND dedupe.activity_event_id = activity.id
             WHERE dedupe.activity_event_id IS NULL"
        )
        .await,
        0,
        "every migrated activity must have its exact dedupe mapping"
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM ingest_dedupes").await,
        scalar(&pool, "SELECT COUNT(*) FROM activity_events").await,
        "migration must produce exact dedupe coverage"
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT id, provider, provider_session_id, provider_turn_id, project_identity, prompt,
                    created_at, origin_id, first_recorded_at_us, first_recorded_at_provenance,
                    global_sequence
             FROM activity_events ORDER BY id",
            11,
        )
        .await,
        activity_facts,
        "replay must not mutate immutable activity facts"
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT id, activity_event_id, position_x, position_y FROM canvas_nodes ORDER BY id",
            4,
        )
        .await,
        canvas_facts,
        "replay must not mutate canvas facts"
    );
}

#[tokio::test]
async fn migration_is_lossless_idempotent_and_truthful() {
    let directory = TempDir::new().expect("database directory");
    let path = directory.path().join("project-context.sqlite");
    let pool = fixture_pool(&path, false).await;
    seed_legacy_fixture(&pool).await;
    let before = legacy_snapshot(&pool).await;
    drop(pool);

    let store = ActivityStore::open(&path).await.expect("store opens");
    store.migrate().await.expect("v2 migration succeeds");
    drop(store);

    let pool = open_pool(&path).await;
    assert_eq!(legacy_snapshot(&pool).await, before);
    assert_eq!(scalar(&pool, "SELECT COUNT(*) FROM projects").await, 3);
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM activity_origins").await,
        3
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM activity_project_assignments").await,
        0
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM conversation_routes").await,
        0
    );
    assert_eq!(
        scalar(
            &pool,
            "SELECT COUNT(*) FROM schema_migrations WHERE version = 2"
        )
        .await,
        1
    );
    assert_eq!(
        scalar(&pool, "SELECT COUNT(*) FROM pragma_foreign_key_check").await,
        0
    );

    assert_eq!(
        rows(
            &pool,
            "SELECT id, identity, display_path, name, normalized_name FROM projects ORDER BY identity",
            5,
        )
        .await,
        vec![
            "10|identity-a|C:\\repos\\same|same|same",
            "20|identity-b|D:\\other\\same|same (2)|same (2)",
            "31|missing-identity||missing-identity|missing-identity",
        ]
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT identity, kind, resolution_source, display_path, routing_mode, setup_state, default_project_id FROM activity_origins ORDER BY identity",
            7,
        )
        .await,
        vec![
            "identity-a|unresolved|legacy_migrated|C:\\repos\\same|dedicated|unconfirmed|10",
            "identity-b|unresolved|legacy_migrated|D:\\other\\same|dedicated|unconfirmed|20",
            "missing-identity|unresolved|legacy_migrated||dedicated|unconfirmed|31",
        ]
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT id, submitted_cwd, captured_at_us, captured_at_provenance, first_recorded_at_us, first_recorded_at_provenance, global_sequence FROM activity_events ORDER BY id",
            7,
        )
        .await,
        vec![
            "101|NULL|NULL|NULL|1735689600000000|legacy_recorded|1",
            "102|NULL|NULL|NULL|NULL|NULL|2",
            "103|NULL|NULL|NULL|1735776000000000|legacy_recorded|3",
        ]
    );
    assert_eq!(
        rows(
            &pool,
            "SELECT a.id, o.identity FROM activity_events a JOIN activity_origins o ON o.id = a.origin_id ORDER BY a.id",
            2,
        )
        .await,
        vec!["101|identity-a", "102|missing-identity", "103|identity-b"]
    );

    let migrated = full_snapshot(&pool).await;
    drop(pool);
    let store = ActivityStore::open(&path).await.expect("store reopens");
    store.migrate().await.expect("second migration succeeds");
    drop(store);
    let pool = open_pool(&path).await;
    assert_eq!(full_snapshot(&pool).await, migrated);
}

#[tokio::test]
async fn fresh_database_reaches_version_two() {
    let store = ActivityStore::in_memory().await.expect("database opens");
    store.migrate().await.expect("fresh migration succeeds");
    store.migrate().await.expect("fresh migration reruns");
}
