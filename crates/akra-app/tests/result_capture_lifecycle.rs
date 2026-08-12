use std::{io::Write, process::Stdio};

use akra_app::{
    capture_gate::CaptureGate,
    recovery::drain,
    spool::{CaptureEnvelope, Spool},
};
use akra_core::ingress::ActivityKind;
use akra_git::ProjectIdentity;
use akra_store::{ActivityScope, ActivityStore, ResultSummaryState, ResultSummaryStatus};
use serde_json::json;
use tempfile::TempDir;

#[tokio::test]
async fn prompt_and_stop_spool_join_into_one_pending_summary() {
    let data = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("work directory");
    CaptureGate::new(data.path())
        .set_enabled(true)
        .expect("capture gate");

    let prompt = json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "summary-session",
        "turn_id": "summary-turn",
        "cwd": cwd.path(),
        "prompt": "결과 요약을 저장해 주세요",
        "model": "gpt-5.3-codex"
    });
    let prompt_output = capture(data.path(), &prompt, &[]);
    assert!(prompt_output.status.success(), "{prompt_output:?}");
    assert!(prompt_output.stdout.is_empty());

    let stop = json!({
        "hook_event_name": "Stop",
        "session_id": "summary-session",
        "turn_id": "summary-turn",
        "cwd": cwd.path(),
        "model": "gpt-5.3-codex",
        "stop_hook_active": false,
        "last_assistant_message": "기능을 구현했고 전체 테스트를 통과했습니다."
    });
    let stop_output = capture(data.path(), &stop, &[]);
    assert!(stop_output.status.success(), "{stop_output:?}");
    assert_eq!(stop_output.stdout, b"{}\n");

    let store = ActivityStore::open(&data.path().join("akra-hookers.sqlite"))
        .await
        .expect("store");
    store.migrate().await.expect("migration");
    let spool = Spool::open(&data.path().join("spool")).expect("spool");
    while !spool.pending().expect("pending").is_empty() {
        assert!(drain(&spool, &store).await > 0);
    }

    let activities = store
        .activity_summaries(ActivityScope::All)
        .await
        .expect("activities");
    assert_eq!(activities.len(), 1);
    assert_eq!(
        activities[0].result_summary_status,
        ResultSummaryStatus::Pending
    );
    let claim = store
        .claim_result_summary(now_us(), 1_000_000)
        .await
        .expect("claim")
        .expect("pending result");
    assert_eq!(
        claim.source_text(),
        "기능을 구현했고 전체 테스트를 통과했습니다."
    );
}

#[tokio::test]
async fn recovery_uses_the_linked_user_activity_instead_of_the_stop_hint() {
    let data = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("work directory");
    let spool = Spool::open(&data.path().join("spool")).expect("spool");
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin")
        .origin;
    let captured_at_us = now_us();

    spool
        .enqueue_envelope(
            &CaptureEnvelope::new_with_activity(
                "codex",
                captured_at_us,
                origin.clone(),
                json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": "authority-session",
                    "turn_id": "user-turn",
                    "cwd": cwd.path(),
                    "prompt": "사용자 프롬프트",
                    "model": "gpt-5.3-codex"
                }),
                ActivityKind::User,
                None,
                None,
            )
            .expect("prompt envelope"),
        )
        .expect("prompt spools");
    assert_eq!(drain(&spool, &store).await, 1);

    spool
        .enqueue_envelope(
            &CaptureEnvelope::new_with_activity(
                "codex",
                captured_at_us + 1,
                origin,
                json!({
                    "hook_event_name": "Stop",
                    "session_id": "authority-session",
                    "turn_id": "user-turn",
                    "cwd": cwd.path(),
                    "last_assistant_message": "DB에 연결된 사용자 결과"
                }),
                ActivityKind::Internal,
                None,
                None,
            )
            .expect("stop envelope"),
        )
        .expect("stop spools");
    assert_eq!(drain(&spool, &store).await, 1);

    let claim = store
        .claim_result_summary(captured_at_us + 1, 1_000_000)
        .await
        .expect("claim")
        .expect("the linked user result remains claimable");
    assert_eq!(claim.source_text(), "DB에 연결된 사용자 결과");
}

#[tokio::test]
async fn recovery_skips_a_linked_non_user_even_when_the_stop_hint_is_user() {
    let data = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("work directory");
    let spool = Spool::open(&data.path().join("spool")).expect("spool");
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin")
        .origin;
    let captured_at_us = now_us();

    spool
        .enqueue_envelope(
            &CaptureEnvelope::new_with_activity(
                "codex",
                captured_at_us,
                origin.clone(),
                json!({
                    "hook_event_name": "UserPromptSubmit",
                    "session_id": "authority-session",
                    "turn_id": "internal-turn",
                    "cwd": cwd.path(),
                    "prompt": "내부 프롬프트",
                    "model": "gpt-5.3-codex"
                }),
                ActivityKind::Internal,
                None,
                None,
            )
            .expect("internal envelope"),
        )
        .expect("internal prompt spools");
    assert_eq!(drain(&spool, &store).await, 1);
    let activity_id = store
        .activity_summaries(ActivityScope::All)
        .await
        .expect("activities")[0]
        .id;

    spool
        .enqueue_envelope(
            &CaptureEnvelope::new_with_activity(
                "codex",
                captured_at_us + 1,
                origin,
                json!({
                    "hook_event_name": "Stop",
                    "session_id": "authority-session",
                    "turn_id": "internal-turn",
                    "cwd": cwd.path(),
                    "last_assistant_message": "저장되면 안 되는 내부 결과"
                }),
                ActivityKind::User,
                None,
                None,
            )
            .expect("stop envelope"),
        )
        .expect("stop spools");
    assert_eq!(drain(&spool, &store).await, 1);

    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("skipped summary state");
    assert_eq!(summary.state, ResultSummaryState::Skipped);
    assert!(!summary.source_retained);
    assert!(
        store
            .claim_result_summary(captured_at_us + 1, 1_000_000)
            .await
            .expect("claim")
            .is_none()
    );
}

#[tokio::test]
async fn expired_raw_result_is_scrubbed_from_spool_before_database_recovery() {
    let data = TempDir::new().expect("data directory");
    let cwd = TempDir::new().expect("work directory");
    let spool = Spool::open(&data.path().join("spool")).expect("spool");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd.path())
        .expect("origin")
        .origin;
    let result = CaptureEnvelope::new(
        "codex",
        0,
        origin,
        json!({
            "hook_event_name": "Stop",
            "session_id": "expired-session",
            "turn_id": "expired-turn",
            "cwd": cwd.path(),
            "model": "gpt-5.3-codex",
            "stop_hook_active": false,
            "last_assistant_message": "This raw result must not outlive retention."
        }),
    )
    .expect("result envelope");
    spool.enqueue_envelope(&result).expect("result spools");
    let store = ActivityStore::open(&data.path().join("akra-hookers.sqlite"))
        .await
        .expect("store");
    store.migrate().await.expect("migration");

    assert_eq!(drain(&spool, &store).await, 0);
    assert!(spool.pending().expect("pending").is_empty());
}

fn now_us() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_micros(),
    )
    .expect("timestamp")
}

#[test]
fn capture_errors_never_use_stop_continuation_exit_code() {
    let data = TempDir::new().expect("data directory");
    CaptureGate::new(data.path())
        .set_enabled(true)
        .expect("capture gate");
    let output = capture_bytes(data.path(), b"{", &[]);
    assert!(!output.status.success());
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn summary_child_guard_bypasses_capture_before_gate_or_payload_parsing() {
    let data = TempDir::new().expect("data directory");
    let output = capture_bytes(
        data.path(),
        b"not json",
        &[(akra_app::summarization::SUMMARY_CHILD_ENV, "1")],
    );
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"{}\n");
    assert!(!data.path().join("spool").exists());
}

fn capture(
    data_dir: &std::path::Path,
    payload: &serde_json::Value,
    env: &[(&str, &str)],
) -> std::process::Output {
    capture_bytes(
        data_dir,
        serde_json::to_string(payload).expect("payload").as_bytes(),
        env,
    )
}

fn capture_bytes(
    data_dir: &std::path::Path,
    payload: &[u8],
    env: &[(&str, &str)],
) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_akra-hookers"));
    command
        .args(["capture", "--data-dir"])
        .arg(data_dir)
        .envs(env.iter().copied())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = command.spawn().expect("capture starts");
    child
        .stdin
        .take()
        .expect("stdin")
        .write_all(payload)
        .expect("payload writes");
    child.wait_with_output().expect("capture exits")
}
