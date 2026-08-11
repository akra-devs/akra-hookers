use akra_adapters::codex::CodexAdapter;

#[test]
fn normalizes_the_documented_user_prompt_submit_fixture() {
    let fixture = include_str!("fixtures/codex-user-prompt-submit.json");
    let event = CodexAdapter::normalize(fixture).expect("fixture normalizes");

    assert_eq!(event.provider().as_str(), "codex");
    assert_eq!(event.prompt(), "add a health check");
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
