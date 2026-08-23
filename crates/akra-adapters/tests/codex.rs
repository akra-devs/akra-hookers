use akra_adapters::codex::{CodexAdapter, CodexCapture};
use akra_core::ingress::ActivityKind;
use serde_json::Value;

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
fn normalizes_stop_with_the_final_assistant_result() {
    let capture = CodexAdapter::normalize_capture(
        r#"{
            "hook_event_name":"Stop",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "cwd":"C:\\dev\\project",
            "model":"gpt-5.3-codex",
            "last_assistant_message":"Implemented the feature and verified the tests."
        }"#,
    )
    .expect("Stop fixture normalizes");

    let CodexCapture::Result(event) = capture else {
        panic!("Stop must normalize as a result capture");
    };
    assert_eq!(event.provider().as_str(), "codex");
    assert_eq!(event.session_id(), "session-1");
    assert_eq!(event.turn_id(), "turn-1");
    assert_eq!(event.cwd(), r"C:\dev\project");
    assert_eq!(event.model(), Some("gpt-5.3-codex"));
    assert_eq!(
        event.result(),
        Some("Implemented the feature and verified the tests.")
    );
}

#[test]
fn normalizes_stop_without_a_final_assistant_result() {
    for last_message in ["null", r#""   ""#] {
        let payload = format!(
            r#"{{
                "hook_event_name":"Stop",
                "session_id":"session-1",
                "turn_id":"turn-1",
                "cwd":"/work/project",
                "model":null,
                "last_assistant_message":{last_message}
            }}"#
        );
        let capture = CodexAdapter::normalize_capture(&payload).expect("Stop normalizes");
        let CodexCapture::Result(event) = capture else {
            panic!("Stop must normalize as a result capture");
        };
        assert_eq!(event.result(), None);
    }
}

#[test]
fn existing_prompt_normalizer_still_rejects_stop() {
    let error = CodexAdapter::normalize(
        r#"{
            "hook_event_name":"Stop",
            "session_id":"session-1",
            "turn_id":"turn-1",
            "cwd":"/work/project",
            "last_assistant_message":"done"
        }"#,
    )
    .expect_err("legacy normalizer accepts only activity hooks");

    assert!(
        error
            .to_string()
            .contains("unexpected Codex hook event: Stop")
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

#[test]
fn parsed_value_capture_matches_the_string_api_for_every_supported_hook() {
    for payload in [
        r#"{"hook_event_name":"UserPromptSubmit","session_id":"s","turn_id":"t","cwd":"C:\\x","prompt":"p","model":"test"}"#,
        r#"{"hook_event_name":"SubagentStart","session_id":"s","turn_id":"t","cwd":"C:\\x","agent_id":"a","agent_type":"reviewer"}"#,
        r#"{"hook_event_name":"Stop","session_id":"s","turn_id":"t","cwd":"C:\\x","last_assistant_message":"done"}"#,
    ] {
        let value: Value = serde_json::from_str(payload).expect("hook JSON");

        assert_eq!(
            CodexAdapter::normalize_capture_value(&value).expect("value capture"),
            CodexAdapter::normalize_capture(payload).expect("string capture")
        );
    }
}
