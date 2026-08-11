use super::*;

#[tokio::test]
async fn concurrent_provider_toggles_keep_manifest_gate_and_store_aligned() {
    let home = tempfile::TempDir::new().expect("temporary Codex home");
    let lifecycle = Arc::new(CodexHookLifecycleSet::from_codex_homes([home
        .path()
        .join(".codex")]));
    let capture_gate = CaptureGate::new(home.path());
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let router = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        Arc::clone(&lifecycle),
        r#""C:\tools\akra-hookers.exe" capture --data-dir "C:\data""#.to_owned(),
        capture_gate.clone(),
    );

    let mut toggles = tokio::task::JoinSet::new();
    for enabled in (0..32).map(|index| index % 2 == 0) {
        let router = router.clone();
        toggles.spawn(async move {
            router
                .oneshot(
                    Request::builder()
                        .method("PATCH")
                        .uri("/v1/providers/codex")
                        .header("authorization", "Bearer fixture-token")
                        .header("content-type", "application/json")
                        .body(Body::from(format!(r#"{{"enabled":{enabled}}}"#)))
                        .expect("request"),
                )
                .await
                .expect("response")
        });
    }
    while let Some(result) = toggles.join_next().await {
        assert_eq!(
            result.expect("toggle task").status(),
            StatusCode::NO_CONTENT
        );
    }

    let enabled = store
        .provider_enabled("codex")
        .await
        .expect("provider state");
    assert_eq!(
        lifecycle.is_enabled().expect("manifest state"),
        enabled,
        "global manifest must agree with the store"
    );
    assert_eq!(
        capture_gate.is_enabled().expect("capture gate state"),
        enabled,
        "fast capture gate must agree with the store"
    );
}
