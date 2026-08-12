use akra_adapters::codex::CodexAdapter;
use akra_core::ingress::ActivityKind;

#[test]
fn normalizes_the_documented_user_prompt_submit_fixture() {
    let fixture = include_str!("fixtures/codex-user-prompt-submit.json");
    let event = CodexAdapter::normalize(fixture).expect("fixture normalizes");

    assert_eq!(event.provider().as_str(), "codex");
    assert_eq!(event.prompt(), "add a health check");
}

#[test]
fn normalizes_subagent_start_with_stable_agent_identity() {
    let event = CodexAdapter::normalize(
        r#"{
            "hook_event_name":"SubagentStart",
            "session_id":"parent-session",
            "turn_id":"parent-turn",
            "cwd":"C:\\dev\\project",
            "model":"gpt-5",
            "agent_id":"019ff551-8f05-7f42-a014-b787ede069cc",
            "agent_type":"reviewer"
        }"#,
    )
    .expect("subagent fixture normalizes");

    assert_eq!(event.activity_kind(), ActivityKind::Subagent);
    assert_eq!(
        event.agent_id(),
        Some("019ff551-8f05-7f42-a014-b787ede069cc")
    );
    assert_eq!(event.agent_type(), Some("reviewer"));
    assert_eq!(
        event.turn_id(),
        "parent-turn:subagent:019ff551-8f05-7f42-a014-b787ede069cc"
    );
}

#[test]
fn rejects_payloads_without_prompt() {
    let error = CodexAdapter::normalize(
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","turn_id":"t","cwd":"C:\\x"}"#,
    )
    .expect_err("prompt is required");
    assert!(error.to_string().contains("prompt"));
}

#[test]
fn rejects_payloads_without_required_technical_ids() {
    for (field, payload) in [
        (
            "session_id",
            r#"{"hook_event_name":"UserPromptSubmit","turn_id":"t","cwd":"C:\\x","prompt":"p"}"#,
        ),
        (
            "turn_id",
            r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","cwd":"C:\\x","prompt":"p"}"#,
        ),
    ] {
        let error = CodexAdapter::normalize(payload)
            .expect_err("missing technical ID must reject the capture payload");
        assert!(error.to_string().contains(field), "{error}");
    }
}
