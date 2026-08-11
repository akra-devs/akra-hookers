use std::path::Path;

use akra_core::ingress::IngressEvent;
use akra_store::{ActivityStore, ActivitySummary, RecordActivity};

pub(crate) async fn record_legacy(
    store: &ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), turn, None)
        .expect("legacy event");
    let origin = akra_git::ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("legacy origin")
        .origin;
    store
        .record(RecordActivity::legacy_resolved(event, origin))
        .await
        .expect("legacy record")
}

pub(crate) fn find(summaries: &[ActivitySummary], activity_id: i64) -> &ActivitySummary {
    summaries
        .iter()
        .find(|summary| summary.id == activity_id)
        .expect("summary")
}

pub(crate) fn assert_position(
    summaries: &[ActivitySummary],
    activity_id: i64,
    index: i64,
    total: i64,
) {
    let summary = find(summaries, activity_id);
    assert_eq!(
        (summary.conversation_index, summary.conversation_total),
        (index, total)
    );
}
