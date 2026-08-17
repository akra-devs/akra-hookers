use std::{fs, sync::Arc};

use akra_adapters::codex::CodexHookLifecycleSet;
use akra_app::{
    capture_gate::CaptureGate,
    http::{app, app_with_codex_lifecycle},
};
use axum::{
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use tower::ServiceExt;

async fn test_app() -> axum::Router {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    app("fixture-token", store)
}

#[path = "cases/api_canvas_contracts.rs"]
mod api_canvas_contracts;
#[path = "cases/api_canvas_mutation_contracts.rs"]
mod api_canvas_mutation_contracts;
#[path = "cases/api_ingest_contracts.rs"]
mod api_ingest_contracts;
#[path = "cases/api_provider_concurrency_contracts.rs"]
mod api_provider_concurrency_contracts;
#[path = "cases/api_provider_contracts.rs"]
mod api_provider_contracts;
#[path = "cases/api_provider_manifest_contracts.rs"]
mod api_provider_manifest_contracts;
#[path = "cases/api_work_curation_contracts.rs"]
mod api_work_curation_contracts;

async fn record(
    store: &akra_store::ActivityStore,
    provider: &str,
    session_id: &str,
    turn_id: &str,
    cwd: &str,
    prompt: &str,
) -> Result<i64, akra_store::StoreError> {
    let event =
        akra_core::ingress::IngressEvent::try_new(provider, session_id, turn_id, cwd, prompt, None)
            .expect("valid fixture event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(std::path::Path::new(cwd))
        .expect("fixture origin")
        .origin;
    store
        .record(akra_store::RecordActivity::legacy_resolved(event, origin))
        .await
}
