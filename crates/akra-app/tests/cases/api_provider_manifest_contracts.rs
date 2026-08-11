use super::*;

#[tokio::test]
async fn provider_toggle_synchronizes_the_global_codex_manifest() {
    let home = tempfile::TempDir::new().expect("temporary Codex home");
    let codex_directory = home.path().join(".codex");
    fs::create_dir_all(&codex_directory).expect("Codex directory");
    fs::write(
        codex_directory.join("hooks.json"),
        r#"{
          "hooks": {
            "UserPromptSubmit": [{
              "hooks": [{
                "type": "command",
                "command": "third-party-hook"
              }]
            }]
          }
        }"#,
    )
    .expect("third-party hook");

    let command = r#""C:\tools\akra-hookers.exe" capture --data-dir "C:\data""#;
    let lifecycle = Arc::new(CodexHookLifecycleSet::from_codex_homes([home
        .path()
        .join(".codex")]));
    let capture_gate = CaptureGate::new(home.path());
    lifecycle.enable(command).expect("initial hook");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let first_activity = record(
        &store,
        "codex",
        "session",
        "turn",
        "project",
        "history survives",
    )
    .await
    .expect("history");
    let second_activity = record(
        &store,
        "codex",
        "session",
        "turn-2",
        "project",
        "connected history survives",
    )
    .await
    .expect("connected history");
    let initial_nodes = store.canvas_nodes().await.expect("canvas");
    let first_node = initial_nodes
        .iter()
        .find(|node| node.activity_event_id == first_activity)
        .expect("first canvas node")
        .id;
    let second_node = initial_nodes
        .iter()
        .find(|node| node.activity_event_id == second_activity)
        .expect("second canvas node")
        .id;
    store
        .update_canvas_position(first_node, 137.0, 251.0)
        .await
        .expect("position");
    store
        .create_canvas_edge(first_node, second_node)
        .await
        .expect("edge");
    let expected_nodes = store
        .canvas_nodes()
        .await
        .expect("canvas")
        .into_iter()
        .map(|node| {
            (
                node.id,
                node.activity_event_id,
                node.position_x,
                node.position_y,
            )
        })
        .collect::<Vec<_>>();
    let expected_edges = store
        .canvas_edges()
        .await
        .expect("canvas edges")
        .into_iter()
        .map(|edge| (edge.id, edge.source_node_id, edge.target_node_id))
        .collect::<Vec<_>>();

    let off = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        Arc::clone(&lifecycle),
        command.to_owned(),
        capture_gate.clone(),
    )
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
    assert_eq!(off.status(), StatusCode::NO_CONTENT);
    assert_eq!(store.activity_count().await.expect("history"), 2);
    assert_eq!(
        store
            .canvas_nodes()
            .await
            .expect("canvas")
            .into_iter()
            .map(|node| {
                (
                    node.id,
                    node.activity_event_id,
                    node.position_x,
                    node.position_y,
                )
            })
            .collect::<Vec<_>>(),
        expected_nodes
    );
    assert_eq!(
        store
            .canvas_edges()
            .await
            .expect("canvas edges")
            .into_iter()
            .map(|edge| (edge.id, edge.source_node_id, edge.target_node_id))
            .collect::<Vec<_>>(),
        expected_edges
    );
    assert!(!capture_gate.is_enabled().expect("capture gate off"));

    let after_off: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(codex_directory.join("hooks.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    let after_off_commands = manifest_commands(&after_off);
    assert_eq!(after_off_commands, vec!["third-party-hook"]);

    let on = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        lifecycle,
        command.to_owned(),
        capture_gate.clone(),
    )
    .oneshot(
        Request::builder()
            .method("PATCH")
            .uri("/v1/providers/codex")
            .header("authorization", "Bearer fixture-token")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"enabled":true}"#))
            .expect("request"),
    )
    .await
    .expect("response");
    assert_eq!(on.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        store
            .canvas_nodes()
            .await
            .expect("canvas")
            .into_iter()
            .map(|node| {
                (
                    node.id,
                    node.activity_event_id,
                    node.position_x,
                    node.position_y,
                )
            })
            .collect::<Vec<_>>(),
        expected_nodes
    );
    assert_eq!(
        store
            .canvas_edges()
            .await
            .expect("canvas edges")
            .into_iter()
            .map(|edge| (edge.id, edge.source_node_id, edge.target_node_id))
            .collect::<Vec<_>>(),
        expected_edges
    );
    assert!(capture_gate.is_enabled().expect("capture gate on"));

    let after_on: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(codex_directory.join("hooks.json")).expect("manifest"),
    )
    .expect("manifest JSON");
    let after_on_commands = manifest_commands(&after_on);
    assert_eq!(
        after_on_commands,
        vec!["third-party-hook", command],
        "ON restores only one bounded synchronous akra hook"
    );
    let hooks = &after_on["hooks"]["UserPromptSubmit"];
    let akra_hook = &hooks[1]["hooks"][0];
    assert!(
        akra_hook.get("async").is_none(),
        "Codex skips unsupported async hooks"
    );
    assert_eq!(akra_hook["timeout"], 1);
}

#[tokio::test]
async fn disable_repairs_a_preexisting_split_multi_home_manifest() {
    let homes = tempfile::TempDir::new().expect("temporary homes");
    let first_home = homes.path().join("first");
    let second_home = homes.path().join("second");
    fs::create_dir_all(&first_home).expect("first home");
    fs::create_dir_all(&second_home).expect("second home");
    fs::write(
        first_home.join("hooks.json"),
        r#"{
          "hooks": {
            "UserPromptSubmit": [{
              "hooks": [
                { "type": "command", "command": "third-party-hook" },
                { "type": "command", "command": "akra-hookers capture" }
              ]
            }]
          }
        }"#,
    )
    .expect("split first manifest");
    fs::write(
        second_home.join("hooks.json"),
        r#"{
          "hooks": {
            "UserPromptSubmit": [{
              "hooks": [{ "type": "command", "command": "second-party-hook" }]
            }]
          }
        }"#,
    )
    .expect("split second manifest");
    let lifecycle = Arc::new(CodexHookLifecycleSet::from_codex_homes([
        first_home.clone(),
        second_home.clone(),
    ]));
    assert!(!lifecycle.is_enabled().expect("split state"));
    let capture_gate = CaptureGate::new(homes.path());
    capture_gate.set_enabled(true).expect("stale capture gate");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");

    let response = app_with_codex_lifecycle(
        "fixture-token",
        store,
        Arc::clone(&lifecycle),
        "akra-hookers capture".to_owned(),
        capture_gate.clone(),
    )
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
    assert!(!capture_gate.is_enabled().expect("capture gate disabled"));
    assert_eq!(
        manifest_commands(&read_manifest(&first_home.join("hooks.json"))),
        vec!["third-party-hook"]
    );
    assert_eq!(
        manifest_commands(&read_manifest(&second_home.join("hooks.json"))),
        vec!["second-party-hook"]
    );
}

#[tokio::test]
async fn lifecycle_gate_is_authoritative_for_provider_status_and_ingest() {
    let home = tempfile::TempDir::new().expect("temporary home");
    let lifecycle = Arc::new(CodexHookLifecycleSet::from_codex_homes([home
        .path()
        .join(".codex")]));
    let capture_gate = CaptureGate::new(home.path());
    capture_gate.set_enabled(false).expect("disabled gate");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    assert!(
        store
            .provider_enabled("codex")
            .await
            .expect("legacy store default")
    );
    let router = app_with_codex_lifecycle(
        "fixture-token",
        Arc::clone(&store),
        lifecycle,
        "akra-hookers capture".to_owned(),
        capture_gate,
    );

    let status = router
        .clone()
        .oneshot(
            Request::builder()
                .uri("/v1/providers/codex")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("status response");
    assert_eq!(status.status(), StatusCode::OK);
    let status_body = to_bytes(status.into_body(), usize::MAX)
        .await
        .expect("status body");
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&status_body).expect("status JSON")["enabled"],
        false
    );

    let ingest = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/ingest")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "session_id": "disabled-session",
                        "turn_id": "disabled-turn",
                        "cwd": home.path(),
                        "prompt": "must not ingest",
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("ingest response");
    assert_eq!(ingest.status(), StatusCode::ACCEPTED);
    assert!(
        store
            .activity_summaries(akra_store::ActivityScope::All)
            .await
            .expect("activities")
            .is_empty()
    );
}

fn read_manifest(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(path).expect("manifest")).expect("valid manifest")
}

fn manifest_commands(manifest: &serde_json::Value) -> Vec<&str> {
    manifest["hooks"]["UserPromptSubmit"]
        .as_array()
        .expect("prompt submit groups")
        .iter()
        .flat_map(|group| group["hooks"].as_array().expect("hook commands"))
        .map(|hook| hook["command"].as_str().expect("command"))
        .collect()
}
