use akra_core::ingress::IngressEvent;

#[test]
fn accepts_complete_codex_prompt_submission() {
    let event = IngressEvent::try_new(
        "codex",
        "session-1",
        "turn-1",
        r"C:\dev\project",
        "add a health check",
        Some("gpt-5.6".to_owned()),
    )
    .expect("complete Codex submissions are valid");

    assert_eq!(event.provider().as_str(), "codex");
    assert_eq!(event.prompt(), "add a health check");
}

#[test]
fn rejects_blank_prompt() {
    let error = IngressEvent::try_new("codex", "session-1", "turn-1", r"C:\dev\project", " ", None)
        .expect_err("blank prompt must be rejected");

    assert_eq!(error.to_string(), "prompt must not be blank");
}
