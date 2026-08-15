use akra_core::{
    ingress::{ActivityKind, IngressEvent, ResultEvent},
    prompt_projection::PromptProjection,
};
use akra_git::ProjectIdentity;
use akra_store::{
    ActivityStore, PromptSummaryCompletionOutcome, PromptSummaryFailureDisposition,
    PromptSummaryPolicy, PromptSummaryState, PromptSummaryStatus, PromptSummaryText,
    RecordActivity, RecordResult, ResultSummaryLines,
};

#[tokio::test]
async fn a_contextual_prompt_uses_the_previous_three_line_result_without_mutating_raw_prompt() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");

    let first = record(&store, "first", "설치와 검증을 진행해 주세요", 10).await;
    store
        .capture_result(result("first", "implementation finished", 11))
        .await
        .expect("result");
    let result_claim = store
        .claim_result_summary(11, 20)
        .await
        .expect("result claim")
        .expect("result claim exists");
    store
        .complete_result_summary(
            &result_claim,
            &result_lines("hook 설치", "검증 완료", "다음 단계 대기"),
            12,
        )
        .await
        .expect("result completion");

    let next = record(&store, "next", "진행해", 20).await;
    let pending = store
        .prompt_summary(next)
        .await
        .expect("prompt summary")
        .expect("initialized");
    assert_eq!(pending.state, PromptSummaryState::Pending);
    assert!(pending.used_previous_result);
    assert_eq!(pending.context_activity_event_id, Some(first));

    let claim = store
        .claim_prompt_summary(20, 20)
        .await
        .expect("prompt claim")
        .expect("contextual prompt claim");
    assert_eq!(claim.activity_event_id(), next);
    assert_eq!(claim.projected_prompt(), "진행해");
    assert_eq!(
        claim.previous_result_lines().expect("previous result"),
        &[
            "hook 설치".to_owned(),
            "검증 완료".to_owned(),
            "다음 단계 대기".to_owned()
        ]
    );
    assert_eq!(
        store
            .complete_prompt_summary(
                &claim,
                &PromptSummaryText::try_new("설치와 검증의 다음 단계를 진행").expect("text"),
                21,
            )
            .await
            .expect("prompt completion"),
        PromptSummaryCompletionOutcome::Applied
    );

    let detail = store.activity_detail(next).await.expect("detail");
    assert_eq!(detail.prompt, "진행해");
    assert_eq!(detail.prompt_summary.status, PromptSummaryStatus::Ready);
    assert_eq!(
        detail.prompt_summary.text.as_deref(),
        Some("설치와 검증의 다음 단계를 진행")
    );
}

#[tokio::test]
async fn a_waiting_context_wakes_once_the_previous_result_becomes_ready() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");
    let first = record(&store, "first", "작업을 수행해 주세요", 10).await;
    store
        .capture_result(result("first", "result is pending", 11))
        .await
        .expect("result");
    let next = record(&store, "next", "그렇게 해주세요", 20).await;
    assert_eq!(
        store
            .prompt_summary(next)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::WaitingContext
    );
    assert!(
        store
            .claim_prompt_summary(20, 20)
            .await
            .expect("claim")
            .is_none()
    );

    let result_claim = store
        .claim_result_summary(21, 20)
        .await
        .expect("result claim")
        .expect("claim");
    assert_eq!(result_claim.activity_event_id(), first);
    store
        .complete_result_summary(
            &result_claim,
            &result_lines("첫 줄", "둘째 줄", "셋째 줄"),
            22,
        )
        .await
        .expect("result completion");
    assert_eq!(
        store
            .prompt_summary(next)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Pending
    );
}

#[tokio::test]
async fn a_policy_change_only_affects_future_activities() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");
    let first = record(&store, "first-policy", "작업을 수행해 주세요", 10).await;
    store
        .capture_result(result("first-policy", "result is pending", 11))
        .await
        .expect("result");
    let next = record(&store, "next-policy", "진행해", 20).await;
    assert_eq!(
        store
            .prompt_summary(next)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::WaitingContext
    );

    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Off)
        .await
        .expect("disable smart summaries");
    let result_claim = store
        .claim_result_summary(21, 20)
        .await
        .expect("result claim")
        .expect("claim");
    assert_eq!(result_claim.activity_event_id(), first);
    store
        .complete_result_summary(
            &result_claim,
            &result_lines("첫 줄", "둘째 줄", "셋째 줄"),
            22,
        )
        .await
        .expect("result completion");

    assert_eq!(
        store
            .prompt_summary(next)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Pending
    );
    let later = record(&store, "later-policy", &"가".repeat(300), 30).await;
    assert_eq!(
        store
            .prompt_summary(later)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Passthrough
    );
}

#[tokio::test]
async fn a_late_predecessor_reconciles_its_immediate_successor_when_the_result_is_ready() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");

    let successor = record(&store, "late-successor", "진행해", 20).await;
    assert_eq!(
        store
            .prompt_summary(successor)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Passthrough
    );

    let predecessor = record(&store, "late-predecessor", "먼저 설치를 준비해 주세요", 10).await;
    let waiting_for_result = store
        .prompt_summary(successor)
        .await
        .expect("summary")
        .expect("row");
    assert_eq!(waiting_for_result.state, PromptSummaryState::Passthrough);
    assert_eq!(
        waiting_for_result.context_activity_event_id,
        Some(predecessor)
    );

    store
        .capture_result(result("late-predecessor", "installation prepared", 21))
        .await
        .expect("result");
    assert_eq!(
        store
            .prompt_summary(successor)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::WaitingContext
    );
    let result_claim = store
        .claim_result_summary(22, 20)
        .await
        .expect("result claim")
        .expect("claim");
    store
        .complete_result_summary(
            &result_claim,
            &result_lines("설치 준비", "hook 경로 확인", "검증 대기"),
            23,
        )
        .await
        .expect("result completion");

    let pending = store
        .prompt_summary(successor)
        .await
        .expect("summary")
        .expect("row");
    assert_eq!(pending.state, PromptSummaryState::Pending);
    assert_eq!(pending.context_activity_event_id, Some(predecessor));
    let claim = store
        .claim_prompt_summary(24, 20)
        .await
        .expect("prompt claim")
        .expect("claim");
    assert_eq!(claim.activity_event_id(), successor);
    assert_eq!(
        claim.previous_result_lines().expect("result lines"),
        &[
            "설치 준비".to_owned(),
            "hook 경로 확인".to_owned(),
            "검증 대기".to_owned(),
        ],
    );
}

#[tokio::test]
async fn off_and_non_user_activities_never_produce_claims() {
    let store = migrated_store().await;
    let off = record(&store, "off", &"가".repeat(300), 10).await;
    assert_eq!(
        store
            .prompt_summary(off)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Passthrough
    );

    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");
    let cwd = std::env::current_dir().expect("cwd");
    let event = IngressEvent::try_new(
        "codex",
        "prompt-summary-session",
        "subagent",
        cwd.to_string_lossy(),
        "진행해",
        None,
    )
    .expect("event")
    .with_activity_context(
        ActivityKind::Subagent,
        Some("agent-1".to_owned()),
        Some("reviewer".to_owned()),
    )
    .expect("context");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
        .expect("origin")
        .origin;
    let subagent = store
        .record(RecordActivity::captured(event, origin, 11))
        .await
        .expect("subagent");
    assert_eq!(
        store
            .prompt_summary(subagent)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Passthrough
    );
    assert!(
        store
            .claim_prompt_summary(12, 20)
            .await
            .expect("no claims")
            .is_none()
    );
}

#[tokio::test]
async fn prompt_summary_failures_retry_once_then_become_terminal() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");
    let activity = record(&store, "long", &"가".repeat(300), 10).await;
    let first = store
        .claim_prompt_summary(10, 10)
        .await
        .expect("claim")
        .expect("first claim");
    assert_eq!(
        store
            .fail_prompt_summary(
                &first,
                Some(20),
                akra_store::PromptSummaryErrorCode::InvalidOutput,
                11,
            )
            .await
            .expect("retry"),
        PromptSummaryFailureDisposition::RetryScheduled
    );
    let second = store
        .claim_prompt_summary(20, 10)
        .await
        .expect("claim")
        .expect("second claim");
    assert_eq!(
        store
            .fail_prompt_summary(
                &second,
                Some(30),
                akra_store::PromptSummaryErrorCode::InvalidOutput,
                21,
            )
            .await
            .expect("terminal failure"),
        PromptSummaryFailureDisposition::Failed
    );
    assert_eq!(
        store
            .prompt_summary(activity)
            .await
            .expect("summary")
            .expect("row")
            .state,
        PromptSummaryState::Failed
    );
}

#[tokio::test]
async fn a_matching_standalone_input_reuses_a_verified_summary_without_another_claim() {
    let store = migrated_store().await;
    store
        .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
        .await
        .expect("enable smart summaries");
    let prompt = "가".repeat(300);
    let first = record_in_session(&store, "cache-one", "cache-session-one", &prompt, 10).await;
    let first_claim = store
        .claim_prompt_summary(10, 10)
        .await
        .expect("claim")
        .expect("first claim");
    store
        .complete_prompt_summary(
            &first_claim,
            &PromptSummaryText::try_new("긴 요청의 핵심 작업을 정리해 진행").expect("text"),
            11,
        )
        .await
        .expect("completion");

    let second = record_in_session(&store, "cache-two", "cache-session-two", &prompt, 20).await;
    let summary = store
        .prompt_summary(second)
        .await
        .expect("summary")
        .expect("row");
    assert_eq!(summary.state, PromptSummaryState::Succeeded);
    assert_eq!(
        summary.text.expect("cached text").as_str(),
        "긴 요청의 핵심 작업을 정리해 진행"
    );
    assert!(
        store
            .claim_prompt_summary(20, 10)
            .await
            .expect("claim")
            .is_none()
    );
    assert_ne!(first, second);
}

async fn migrated_store() -> ActivityStore {
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migrations");
    store
}

async fn record(store: &ActivityStore, turn: &str, prompt: &str, captured_at_us: i64) -> i64 {
    record_in_session(
        store,
        turn,
        "prompt-summary-session",
        prompt,
        captured_at_us,
    )
    .await
}

async fn record_in_session(
    store: &ActivityStore,
    turn: &str,
    session: &str,
    prompt: &str,
    captured_at_us: i64,
) -> i64 {
    let cwd = std::env::current_dir().expect("cwd");
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), prompt, None)
        .expect("event");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
        .expect("origin")
        .origin;
    store
        .record(
            RecordActivity::captured(event, origin, captured_at_us)
                .with_prompt_projection(PromptProjection::raw(prompt)),
        )
        .await
        .expect("record")
}

fn result(turn: &str, text: &str, captured_at_us: i64) -> RecordResult {
    RecordResult::captured(
        ResultEvent::try_new(
            "codex",
            "prompt-summary-session",
            turn,
            std::env::current_dir().expect("cwd").to_string_lossy(),
            Some(text.to_owned()),
            None,
        )
        .expect("result"),
        captured_at_us,
    )
}

fn result_lines(first: &str, second: &str, third: &str) -> ResultSummaryLines {
    ResultSummaryLines::try_new(first, second, third).expect("result lines")
}
