use axum::http::{Method, StatusCode};

#[path = "support/api_harness.rs"]
mod api_harness;
use api_harness::{call, harness};

#[tokio::test]
async fn dashboard_can_toggle_contextual_prompt_summaries_without_touching_capture_hooks() {
    let harness = harness().await;

    let (initial_status, initial) =
        call(&harness.app, Method::GET, "/v1/providers/codex", None, true).await;
    assert_eq!(initial_status, StatusCode::OK);
    assert_eq!(initial["prompt_summary_mode"], "off");

    assert_eq!(
        call(
            &harness.app,
            Method::PUT,
            "/v1/providers/codex/prompt-summaries",
            Some(serde_json::json!({"mode": "smart"})),
            true,
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    let (enabled_status, enabled) =
        call(&harness.app, Method::GET, "/v1/providers/codex", None, true).await;
    assert_eq!(enabled_status, StatusCode::OK);
    assert_eq!(enabled["prompt_summary_mode"], "smart");

    let (invalid_status, invalid) = call(
        &harness.app,
        Method::PUT,
        "/v1/providers/codex/prompt-summaries",
        Some(serde_json::json!({"mode": "always"})),
        true,
    )
    .await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(invalid["code"], "invalid_prompt_summary_mode");
    assert_eq!(
        call(
            &harness.app,
            Method::PUT,
            "/v1/providers/codex/prompt-summaries",
            Some(serde_json::json!({"mode": "off"})),
            false,
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
}
