use akra_core::ingress::{ActivityKind, IngressEvent, ResultEvent};
use akra_git::ProjectIdentity;
use akra_store::{
    ActivityStore, MAX_RESULT_SOURCE_RETENTION_US, MAX_RESULT_SUMMARY_CHARS, RecordActivity,
    RecordResult, ResultCaptureOutcome, ResultSummaryFailureDisposition, ResultSummaryLines,
    ResultSummaryState, ResultSummaryStatus, ResultSummaryValidationError,
};

#[test]
fn summary_lines_enforce_one_shared_180_character_budget() {
    let sixty = "가".repeat(60);
    let accepted = ResultSummaryLines::try_new(&sixty, &sixty, &sixty).expect("exact budget");
    assert_eq!(
        accepted
            .as_array()
            .iter()
            .map(|line| line.chars().count())
            .sum::<usize>(),
        MAX_RESULT_SUMMARY_CHARS
    );

    assert_eq!(
        ResultSummaryLines::try_new("가".repeat(179), "나", "다"),
        Err(ResultSummaryValidationError::SummaryTooLong(181))
    );
}

#[test]
fn summary_budget_uses_trimmed_unicode_scalars_and_rejects_invalid_lines() {
    let accepted = ResultSummaryLines::try_new(
        format!("\u{2003}{}\u{2003}", "😀".repeat(178)),
        "e",
        "\u{301}",
    )
    .expect("178 astral scalars plus two scalar values");
    assert_eq!(accepted.as_array()[0].chars().count(), 178);
    assert_eq!(
        accepted
            .as_array()
            .iter()
            .map(|line| line.chars().count())
            .sum::<usize>(),
        MAX_RESULT_SUMMARY_CHARS
    );
    assert_eq!(
        ResultSummaryLines::try_new(" ", "second", "third"),
        Err(ResultSummaryValidationError::BlankLine(1))
    );
    assert_eq!(
        ResultSummaryLines::try_new("first", "second\nline", "third"),
        Err(ResultSummaryValidationError::EmbeddedNewline(2))
    );
}

#[tokio::test]
async fn result_can_arrive_before_or_after_its_user_prompt() {
    let store = migrated_store().await;

    assert_eq!(
        store
            .capture_result(result("before", "result before prompt", 10))
            .await
            .expect("capture"),
        ResultCaptureOutcome::Inserted
    );
    assert!(
        store
            .claim_result_summary(10, 50)
            .await
            .expect("claim")
            .is_none(),
        "an unlinked result must not be summarized"
    );
    let before_id = record(&store, "before", ActivityKind::User).await;
    let before_claim = store
        .claim_result_summary(11, 50)
        .await
        .expect("claim")
        .expect("linked result");
    assert_eq!(before_claim.activity_event_id(), before_id);
    assert_eq!(before_claim.source_text(), "result before prompt");
    assert!(
        store
            .complete_result_summary(&before_claim, &lines("one", "two", "three"), 12)
            .await
            .expect("complete")
    );
    let detail = store
        .activity_detail(before_id)
        .await
        .expect("detail with summary");
    assert_eq!(detail.result_summary.status, ResultSummaryStatus::Ready);
    assert_eq!(
        detail.result_summary.lines,
        Some(["one".to_owned(), "two".to_owned(), "three".to_owned()])
    );
    assert_eq!(detail.selected_turn.result_summary, detail.result_summary);

    let after_id = record(&store, "after", ActivityKind::User).await;
    store
        .capture_result(result("after", "result after prompt", 20))
        .await
        .expect("capture");
    let after_claim = store
        .claim_result_summary(20, 50)
        .await
        .expect("claim")
        .expect("prompt-first result");
    assert_eq!(after_claim.activity_event_id(), after_id);
}

#[tokio::test]
async fn an_unlinked_result_is_scrubbed_after_the_bounded_retention_window() {
    let store = migrated_store().await;
    store
        .capture_result(result("orphan", "unmatched result", 1))
        .await
        .expect("capture");
    assert!(
        store
            .claim_result_summary(MAX_RESULT_SOURCE_RETENTION_US + 2, 50)
            .await
            .expect("sweep")
            .is_none()
    );

    let activity_id = record(&store, "orphan", ActivityKind::User).await;
    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("expired result state");
    assert_eq!(summary.state, ResultSummaryState::Failed);
    assert!(!summary.source_retained);
}

#[tokio::test]
async fn duplicate_is_idempotent_and_changed_source_invalidates_an_old_lease() {
    let store = migrated_store().await;
    let activity_id = record(&store, "change", ActivityKind::User).await;
    assert_eq!(
        store
            .capture_result(result("change", "first result", 10))
            .await
            .expect("first capture"),
        ResultCaptureOutcome::Inserted
    );
    let old_claim = store
        .claim_result_summary(10, 100)
        .await
        .expect("claim")
        .expect("old claim");
    assert_eq!(
        store
            .capture_result(result("change", "first result", 11))
            .await
            .expect("duplicate capture"),
        ResultCaptureOutcome::Duplicate
    );
    assert_eq!(
        store
            .capture_result(result("change", "changed result", 12))
            .await
            .expect("changed capture"),
        ResultCaptureOutcome::Updated
    );
    let new_claim = store
        .claim_result_summary(12, 100)
        .await
        .expect("claim")
        .expect("new claim");
    assert_eq!(new_claim.source_text(), "changed result");
    assert!(
        !store
            .complete_result_summary(&old_claim, &lines("old 1", "old 2", "old 3"), 13)
            .await
            .expect("stale completion")
    );
    assert!(
        store
            .complete_result_summary(&new_claim, &lines("new 1", "new 2", "new 3"), 14)
            .await
            .expect("new completion")
    );
    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("stored summary");
    assert_eq!(summary.generation, 2);
    assert_eq!(summary.attempt_count, 1);
    assert_eq!(summary.state, ResultSummaryState::Succeeded);
    assert!(!summary.source_retained);
    assert_eq!(
        summary.lines.expect("lines").into_array(),
        ["new 1", "new 2", "new 3"]
    );
}

#[tokio::test]
async fn changed_results_cannot_replace_an_equal_or_newer_capture() {
    let store = migrated_store().await;
    let activity_id = record(&store, "reverse", ActivityKind::User).await;
    assert_eq!(
        store
            .capture_result(result("reverse", "newest result", 20))
            .await
            .expect("newest capture"),
        ResultCaptureOutcome::Inserted
    );
    assert_eq!(
        store
            .capture_result(result("reverse", "older result", 10))
            .await
            .expect("older capture"),
        ResultCaptureOutcome::IgnoredStale
    );
    assert_eq!(
        store
            .capture_result(result("reverse", "same-time result", 20))
            .await
            .expect("same-time capture"),
        ResultCaptureOutcome::IgnoredStale
    );

    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("stored state");
    assert_eq!(summary.generation, 1);
    assert_eq!(summary.state, ResultSummaryState::Pending);
    let claim = store
        .claim_result_summary(20, 100)
        .await
        .expect("claim")
        .expect("newest result remains claimable");
    assert_eq!(claim.source_text(), "newest result");
}

#[tokio::test]
async fn retries_are_due_time_bounded_and_terminal_failure_scrubs_source() {
    let store = migrated_store().await;
    let activity_id = record(&store, "retry", ActivityKind::User).await;
    store
        .capture_result(result("retry", "temporary result", 10))
        .await
        .expect("capture");

    let first = store
        .claim_result_summary(10, 10)
        .await
        .expect("claim")
        .expect("first claim");
    assert_eq!(first.attempt_number(), 1);
    assert_eq!(
        store
            .fail_result_summary(&first, "temporary\nerror", Some(30), 20)
            .await
            .expect("retry"),
        ResultSummaryFailureDisposition::RetryScheduled
    );
    assert!(
        store
            .claim_result_summary(29, 10)
            .await
            .expect("early claim")
            .is_none()
    );
    let second = store
        .claim_result_summary(30, 10)
        .await
        .expect("claim")
        .expect("second claim");
    assert_eq!(second.attempt_number(), 2);
    assert_eq!(
        store
            .fail_result_summary(&second, "temporary again", Some(50), 31)
            .await
            .expect("second retry"),
        ResultSummaryFailureDisposition::RetryScheduled
    );
    let third = store
        .claim_result_summary(50, 10)
        .await
        .expect("claim")
        .expect("third claim");
    assert_eq!(third.attempt_number(), 3);
    assert_eq!(
        store
            .fail_result_summary(&third, "permanent error", Some(70), 51)
            .await
            .expect("bounded terminal failure"),
        ResultSummaryFailureDisposition::Failed,
        "a requested retry must become terminal at the attempt limit"
    );
    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("stored state");
    assert_eq!(summary.state, ResultSummaryState::Failed);
    assert!(!summary.source_retained);
    assert!(
        store
            .claim_result_summary(1_000, 10)
            .await
            .expect("claim")
            .is_none()
    );
}

#[tokio::test]
async fn an_expired_lease_is_reclaimed_and_the_old_worker_becomes_stale() {
    let store = migrated_store().await;
    record(&store, "lease", ActivityKind::User).await;
    store
        .capture_result(result("lease", "lease result", 10))
        .await
        .expect("capture");
    let old_claim = store
        .claim_result_summary(10, 10)
        .await
        .expect("claim")
        .expect("old claim");
    assert!(
        store
            .claim_result_summary(19, 10)
            .await
            .expect("claim before expiry")
            .is_none()
    );
    let reclaimed = store
        .claim_result_summary(20, 10)
        .await
        .expect("claim at expiry")
        .expect("reclaimed claim");
    assert_eq!(reclaimed.attempt_number(), 2);
    assert!(
        !store
            .complete_result_summary(&old_claim, &lines("old 1", "old 2", "old 3"), 21)
            .await
            .expect("stale completion")
    );
    assert!(
        store
            .complete_result_summary(&reclaimed, &lines("new 1", "new 2", "new 3"), 22)
            .await
            .expect("completion")
    );
}

#[tokio::test]
async fn non_user_activity_is_skipped_and_never_retains_raw_result() {
    let store = migrated_store().await;
    store
        .capture_result(result("subagent", "internal result", 10))
        .await
        .expect("result first");
    let activity_id = record(&store, "subagent", ActivityKind::Subagent).await;
    let summary = store
        .result_summary(activity_id)
        .await
        .expect("summary")
        .expect("skipped state");
    assert_eq!(summary.state, ResultSummaryState::Skipped);
    assert!(!summary.source_retained);
    assert!(
        store
            .claim_result_summary(10, 50)
            .await
            .expect("claim")
            .is_none()
    );

    let internal_id = record(&store, "internal", ActivityKind::Internal).await;
    store
        .capture_result(result("internal", "another internal result", 20))
        .await
        .expect("prompt-first internal result");
    let internal = store
        .result_summary(internal_id)
        .await
        .expect("summary")
        .expect("skipped state");
    assert_eq!(internal.state, ResultSummaryState::Skipped);
    assert!(!internal.source_retained);
}

async fn migrated_store() -> ActivityStore {
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migrations");
    store
}

async fn record(store: &ActivityStore, turn: &str, activity_kind: ActivityKind) -> i64 {
    let cwd = std::env::current_dir().expect("cwd");
    let event = IngressEvent::try_new(
        "codex",
        "result-summary-session",
        turn,
        cwd.to_string_lossy(),
        format!("prompt {turn}"),
        None,
    )
    .expect("event")
    .with_activity_context(
        activity_kind,
        (activity_kind == ActivityKind::Subagent).then(|| "agent-1".to_owned()),
        (activity_kind == ActivityKind::Subagent).then(|| "reviewer".to_owned()),
    )
    .expect("activity context");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, 1))
        .await
        .expect("record")
}

fn result(turn: &str, text: &str, captured_at_us: i64) -> RecordResult {
    RecordResult::captured(
        ResultEvent::try_new(
            "codex",
            "result-summary-session",
            turn,
            std::env::current_dir().expect("cwd").to_string_lossy(),
            Some(text.to_owned()),
            Some("gpt-5.3-codex".to_owned()),
        )
        .expect("result event"),
        captured_at_us,
    )
}

fn lines(first: &str, second: &str, third: &str) -> ResultSummaryLines {
    ResultSummaryLines::try_new(first, second, third).expect("valid summary")
}
