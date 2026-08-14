use std::{path::PathBuf, sync::Arc};

use akra_app::{
    collector::{CollectorConfigInput, CollectorManager},
    http::app_with_collector,
    recovery::drain,
    spool::{CaptureEnvelope, Spool},
};
use akra_git::{ProjectOriginKind, ProjectOriginSnapshot};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode, header},
};
use serde_json::json;
use tempfile::TempDir;
use tower::ServiceExt;
use uuid::Uuid;

fn prompt_envelope() -> CaptureEnvelope {
    CaptureEnvelope::new_with_source(
        "codex",
        42,
        ProjectOriginSnapshot {
            identity: "source-project".to_owned(),
            kind: ProjectOriginKind::Git,
            display_path: PathBuf::from("C:/source/project"),
        },
        json!({
            "hook_event_name": "UserPromptSubmit",
            "session_id": "source-session",
            "turn_id": "source-turn",
            "cwd": "C:/source/project",
            "prompt": "capture this on the remote collector",
            "model": "gpt-5"
        }),
        "windows-native",
        "cli",
    )
    .expect("valid source envelope")
}

async fn request(
    app: &Router,
    method: &str,
    uri: &str,
    token: Option<&str>,
    body: impl Into<Body>,
) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = token {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    request = request.header(header::CONTENT_TYPE, "application/json");
    let response = app
        .clone()
        .oneshot(request.body(body.into()).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let headers = response.headers().clone();
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body")
        .to_vec();
    (status, headers, body)
}

#[tokio::test]
async fn collector_ingress_is_capability_scoped_idempotent_and_preserves_source_context() {
    let source_dir = TempDir::new().expect("source data directory");
    let source = CollectorManager::open(source_dir.path()).expect("source manager");
    source
        .configure(CollectorConfigInput {
            endpoint: "https://collector.example".to_owned(),
            token: Some("source-to-collector-token".to_owned()),
        })
        .expect("remote source destination");
    source.capture(&prompt_envelope()).expect("remote queue");

    let source_outbox = Spool::open(&source_dir.path().join("remote-outbox")).expect("outbox");
    let queued = source_outbox.pending().expect("queued capture");
    assert_eq!(queued.len(), 1);
    let remote_wire = source_outbox.read(&queued[0]).expect("wire payload");
    assert!(
        !String::from_utf8_lossy(&remote_wire).contains("source-to-collector-token"),
        "access tokens remain in local collector configuration, not capture envelopes"
    );

    let collector_dir = TempDir::new().expect("collector data directory");
    let collector = Arc::new(CollectorManager::open(collector_dir.path()).expect("collector"));
    let collector_token = collector.collector_token().expose_secret().to_owned();
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migrations");
    let app = app_with_collector(
        "dashboard-token",
        Arc::clone(&store),
        Arc::clone(&collector),
    );

    let (status, headers, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some(&collector_token),
        RequestBody::json(&remote_wire),
    )
    .await;
    assert_eq!(status, StatusCode::ACCEPTED);
    assert!(headers.get(header::ACCESS_CONTROL_ALLOW_ORIGIN).is_none());

    let (duplicate_status, _, duplicate_body) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some(&collector_token),
        RequestBody::json(&remote_wire),
    )
    .await;
    assert_eq!(duplicate_status, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&duplicate_body).expect("duplicate body")["status"],
        "duplicate"
    );

    let mut invalid_capture: serde_json::Value =
        serde_json::from_slice(&remote_wire).expect("remote wire JSON");
    invalid_capture["capture_id"] = json!(Uuid::new_v4().to_string());
    invalid_capture["envelope"]["payload"]["prompt"] = json!("");
    let (invalid_status, _, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some(&collector_token),
        Body::from(invalid_capture.to_string()),
    )
    .await;
    assert_eq!(invalid_status, StatusCode::UNPROCESSABLE_ENTITY);

    let (dashboard_with_collector_token, _, _) = request(
        &app,
        "GET",
        "/v1/providers/codex",
        Some(&collector_token),
        Body::empty(),
    )
    .await;
    assert_eq!(dashboard_with_collector_token, StatusCode::UNAUTHORIZED);

    let (collector_with_dashboard_token, _, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some("dashboard-token"),
        RequestBody::json(&remote_wire),
    )
    .await;
    assert_eq!(collector_with_dashboard_token, StatusCode::UNAUTHORIZED);

    let (unauthenticated_oversized, _, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        None,
        vec![b'x'; akra_app::spool::MAX_PENDING_ITEM_BYTES + 1],
    )
    .await;
    assert_eq!(unauthenticated_oversized, StatusCode::UNAUTHORIZED);

    let receiver_spool = Spool::open(&collector_dir.path().join("spool")).expect("receiver spool");
    let pending = receiver_spool.pending().expect("receiver capture");
    assert_eq!(pending.len(), 1, "invalid input was not queued");
    let received = CaptureEnvelope::decode(&receiver_spool.read(&pending[0]).expect("received"))
        .expect("valid received envelope");
    assert_eq!(
        received.capture_source(),
        None,
        "source runtime is not a collector runtime selector"
    );
    let source_id = serde_json::from_slice::<serde_json::Value>(&remote_wire).expect("remote wire")
        ["source_instance_id"]
        .as_str()
        .expect("source instance id")
        .to_owned();
    assert_eq!(
        received.payload()["session_id"],
        format!("remote:{source_id}:source-session")
    );
    assert!(
        received
            .origin()
            .identity
            .starts_with(&format!("remote:{source_id}:"))
    );

    assert_eq!(drain(&receiver_spool, &store).await, 1);
    let activity = store
        .activities()
        .await
        .expect("activity list")
        .pop()
        .expect("remote activity");
    let detail = store
        .activity_detail(activity.id)
        .await
        .expect("activity detail");
    assert_eq!(
        detail.technical.session_id,
        format!("remote:{source_id}:source-session")
    );
    assert_eq!(detail.origin.display_path, "C:/source/project");
}

#[tokio::test]
async fn collector_rejects_a_conflicting_capture_id_without_enqueuing_it_twice() {
    let source_dir = TempDir::new().expect("source data directory");
    let source = CollectorManager::open(source_dir.path()).expect("source manager");
    source
        .configure(CollectorConfigInput {
            endpoint: "https://collector.example".to_owned(),
            token: Some("source-to-collector-token".to_owned()),
        })
        .expect("remote source destination");
    source.capture(&prompt_envelope()).expect("remote queue");
    let source_outbox = Spool::open(&source_dir.path().join("remote-outbox")).expect("outbox");
    let item = source_outbox.pending().expect("pending").remove(0);
    let wire = source_outbox.read(&item).expect("wire");

    let collector_dir = TempDir::new().expect("collector data directory");
    let collector = Arc::new(CollectorManager::open(collector_dir.path()).expect("collector"));
    let token = collector.collector_token().expose_secret().to_owned();
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migrations");
    let app = app_with_collector("dashboard-token", store, collector);

    let (accepted, _, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some(&token),
        RequestBody::json(&wire),
    )
    .await;
    assert_eq!(accepted, StatusCode::ACCEPTED);

    let mut conflict: serde_json::Value = serde_json::from_slice(&wire).expect("wire JSON");
    conflict["envelope"]["payload"]["prompt"] = json!("conflicting payload");
    let (status, _, _) = request(
        &app,
        "POST",
        "/v1/collector/ingest",
        Some(&token),
        Body::from(conflict.to_string()),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);

    let spool = Spool::open(&collector_dir.path().join("spool")).expect("receiver spool");
    assert_eq!(spool.pending().expect("pending captures").len(), 1);
}

#[tokio::test]
async fn dashboard_destination_configuration_never_returns_its_access_token() {
    let directory = TempDir::new().expect("collector data directory");
    let collector = Arc::new(CollectorManager::open(directory.path()).expect("collector"));
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migrations");
    let app = app_with_collector("dashboard-token", store, collector);

    let secret = "remote-collector-secret";
    let (configured, _, _) = request(
        &app,
        "PUT",
        "/v1/providers/codex/collector",
        Some("dashboard-token"),
        Body::from(
            json!({
                "endpoint": "https://collector.example/",
                "token": secret,
            })
            .to_string(),
        ),
    )
    .await;
    assert_eq!(configured, StatusCode::NO_CONTENT);

    let (status, _, body) = request(
        &app,
        "GET",
        "/v1/providers/codex",
        Some("dashboard-token"),
        Body::empty(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let response: serde_json::Value = serde_json::from_slice(&body).expect("provider JSON");
    assert_eq!(response["collector"]["mode"], "remote");
    assert_eq!(
        response["collector"]["endpoint"],
        "https://collector.example"
    );
    assert_eq!(response["collector"]["token_configured"], true);
    assert!(!String::from_utf8_lossy(&body).contains(secret));

    let (rejected, _, _) = request(
        &app,
        "PUT",
        "/v1/providers/codex/collector",
        Some("dashboard-token"),
        Body::from(json!({ "endpoint": "http://collector.example" }).to_string()),
    )
    .await;
    assert_eq!(rejected, StatusCode::UNPROCESSABLE_ENTITY);

    let (after_rejection, _, body) = request(
        &app,
        "GET",
        "/v1/providers/codex",
        Some("dashboard-token"),
        Body::empty(),
    )
    .await;
    assert_eq!(after_rejection, StatusCode::OK);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&body).expect("provider JSON")["collector"]["endpoint"],
        "https://collector.example"
    );
}

struct RequestBody;

impl RequestBody {
    fn json(bytes: &[u8]) -> Body {
        Body::from(bytes.to_vec())
    }
}
