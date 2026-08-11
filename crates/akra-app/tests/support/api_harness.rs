use std::sync::Arc;

use akra_app::http::app;
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Method, Request, StatusCode},
};
use serde_json::Value;
use tower::ServiceExt;

pub(crate) struct Harness {
    pub(crate) app: Router,
    pub(crate) store: Arc<akra_store::ActivityStore>,
}

pub(crate) async fn harness() -> Harness {
    let store = Arc::new(akra_store::ActivityStore::in_memory().await.expect("store"));
    store.migrate().await.expect("migration");
    Harness {
        app: app("fixture-token", Arc::clone(&store)),
        store,
    }
}

pub(crate) async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    body: Option<Value>,
    authorized: bool,
) -> (StatusCode, Value) {
    let mut request = Request::builder().method(method).uri(uri);
    if authorized {
        request = request.header("authorization", "Bearer fixture-token");
    }
    let body = match body {
        Some(value) => {
            request = request.header("content-type", "application/json");
            Body::from(value.to_string())
        }
        None => Body::empty(),
    };
    let response = app
        .clone()
        .oneshot(request.body(body).expect("request"))
        .await
        .expect("response");
    let status = response.status();
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body");
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

pub(crate) async fn create_project(app: &Router, name: &str) -> (StatusCode, Value) {
    call(
        app,
        Method::POST,
        "/v1/projects",
        Some(serde_json::json!({"name": name})),
        true,
    )
    .await
}
