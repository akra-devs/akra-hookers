pub(crate) async fn record_with_origin(
    store: &akra_store::ActivityStore,
    session: &str,
    turn: &str,
    display_path: &std::path::Path,
    identity: &str,
) -> i64 {
    let event = akra_core::ingress::IngressEvent::try_new(
        "codex",
        session,
        turn,
        display_path.to_string_lossy(),
        turn,
        None,
    )
    .expect("event");
    let origin = akra_git::ProjectOriginSnapshot {
        identity: identity.to_owned(),
        kind: akra_git::ProjectOriginKind::Directory,
        display_path: display_path.to_path_buf(),
    };
    store
        .record(akra_store::RecordActivity::captured(event, origin, 1))
        .await
        .expect("record")
}
