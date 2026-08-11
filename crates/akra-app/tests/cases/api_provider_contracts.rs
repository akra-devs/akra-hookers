use super::*;

#[tokio::test]
async fn provider_toggle_changes_future_capture_without_deleting_history() {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    record(&store, "codex", "s", "history", "C:\\x", "keep")
        .await
        .expect("history");

    let response = app("fixture-token", Arc::clone(&store))
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/v1/providers/codex")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"enabled":false}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("history remains"), 1);
    assert!(
        !store
            .provider_enabled("codex")
            .await
            .expect("provider status")
    );
}
