use super::*;

async fn response_json(response: axum::response::Response) -> serde_json::Value {
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    if bytes.is_empty() {
        serde_json::Value::Null
    } else {
        serde_json::from_slice(&bytes).expect("JSON response")
    }
}

#[tokio::test]
async fn curation_and_work_routes_preserve_the_confirmation_boundary() {
    let directory = tempfile::TempDir::new().expect("directory");
    let cwd = directory.path().join("project");
    fs::create_dir(&cwd).expect("project");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let first = record(
        &store,
        "codex",
        "same-session",
        "first",
        &cwd.to_string_lossy(),
        "배포 페이지 공개",
    )
    .await
    .expect("first");
    let second = record(
        &store,
        "codex",
        "same-session",
        "second",
        &cwd.to_string_lossy(),
        "portable 용량 분석",
    )
    .await
    .expect("second");
    let project_id = store
        .activity_detail(first)
        .await
        .expect("detail")
        .project
        .expect("project")
        .id;
    let router = app("fixture-token", Arc::clone(&store));

    let unauthorized = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/curation/logs?project_id={project_id}"))
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);

    let logs = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/curation/logs?project_id={project_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(logs.status(), StatusCode::OK);
    assert_eq!(response_json(logs).await.as_array().expect("logs").len(), 2);

    let unavailable = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/curation/proposals")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "project_id": project_id,
                        "activity_ids": [first, second]
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(
        store
            .work_items(Some(project_id))
            .await
            .expect("works")
            .is_empty(),
        "a failed AI request cannot mutate work memory"
    );

    let preparation = store
        .prepare_curation(project_id, &[first, second])
        .await
        .expect("prepare");
    let groups = vec![
        akra_store::CurationProposalGroup {
            target_work_id: None,
            title: "Windows Portable 배포".into(),
            log_ids: vec![first],
            confidence: 91,
            uncertain: false,
        },
        akra_store::CurationProposalGroup {
            target_work_id: None,
            title: "Portable 용량 최적화".into(),
            log_ids: vec![second],
            confidence: 84,
            uncertain: false,
        },
    ];
    let proposal = store
        .save_curation_proposal(&preparation, groups.clone())
        .await
        .expect("proposal");
    let apply = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/v1/curation/proposals/{}/apply", proposal.id))
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({"groups": groups}).to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(apply.status(), StatusCode::OK);
    let applied = response_json(apply).await;
    assert_eq!(applied["work_ids"].as_array().expect("work ids").len(), 2);

    let works = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/v1/work-items?project_id={project_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(works.status(), StatusCode::OK);
    let works = response_json(works).await;
    let works = works.as_array().expect("works");
    assert_eq!(works.len(), 2);
    let source = works[0]["id"].as_i64().expect("source");
    let target = works[1]["id"].as_i64().expect("target");

    let edge = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/work-items/edges")
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(
                    serde_json::json!({
                        "source_work_item_id": source,
                        "target_work_item_id": target
                    })
                    .to_string(),
                ))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(edge.status(), StatusCode::CREATED);

    let revision = router
        .oneshot(
            Request::builder()
                .uri("/v1/work-items/revision")
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        response_json(revision).await["revision"]
            .as_i64()
            .unwrap_or_default()
            > 0
    );
}

#[tokio::test]
async fn curation_log_mutations_validate_state_and_soft_delete_immediately() {
    let directory = tempfile::TempDir::new().expect("directory");
    let cwd = directory.path().join("project");
    fs::create_dir(&cwd).expect("project");
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    let activity_id = record(
        &store,
        "codex",
        "session",
        "turn",
        &cwd.to_string_lossy(),
        "불필요한 로그",
    )
    .await
    .expect("activity");
    let project_id = store
        .activity_detail(activity_id)
        .await
        .expect("detail")
        .project
        .expect("project")
        .id;
    let router = app("fixture-token", Arc::clone(&store));

    let excluded = router
        .clone()
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri(format!("/v1/curation/logs/{activity_id}"))
                .header("authorization", "Bearer fixture-token")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"excluded":true}"#))
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(excluded.status(), StatusCode::NO_CONTENT);

    let deleted = router
        .clone()
        .oneshot(
            Request::builder()
                .method("DELETE")
                .uri(format!("/v1/curation/logs/{activity_id}"))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    let logs = router
        .oneshot(
            Request::builder()
                .uri(format!(
                    "/v1/curation/logs?project_id={project_id}&state=all"
                ))
                .header("authorization", "Bearer fixture-token")
                .body(Body::empty())
                .expect("request"),
        )
        .await
        .expect("response");
    assert!(
        response_json(logs)
            .await
            .as_array()
            .expect("logs")
            .is_empty()
    );
}
