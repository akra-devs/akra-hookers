use std::{fs, path::Path};

use akra_core::ingress::IngressEvent;
use akra_git::ProjectIdentity;
use akra_store::{
    ActivityStore, ActivityTimeRange, CurationLogFilter, CurationLogState, CurationProposalGroup,
    RecordActivity, StoreError,
};
use tempfile::TempDir;

#[tokio::test]
async fn curation_is_inert_until_apply_and_keeps_logs_as_traceable_evidence() {
    let directory = TempDir::new().expect("directory");
    let cwd = directory.path().join("project");
    fs::create_dir(&cwd).expect("project directory");
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");

    let first = record(
        &store,
        &cwd,
        "same-session",
        "one",
        "portable 배포 페이지 공개",
        10,
    )
    .await;
    let second = record(
        &store,
        &cwd,
        "same-session",
        "two",
        "zip 다운로드 링크 검증",
        20,
    )
    .await;
    let third = record(
        &store,
        &cwd,
        "different-session",
        "three",
        "ffmpeg 용량 원인 분석",
        30,
    )
    .await;
    let excluded = record(&store, &cwd, "different-session", "four", "진행해", 40).await;
    let project_id = project_id(&store, first).await;

    let initial = store
        .curation_logs(project_id, CurationLogFilter::Unreviewed, 100)
        .await
        .expect("initial logs");
    assert_eq!(initial.len(), 4);
    assert!(
        initial
            .iter()
            .all(|log| log.state == CurationLogState::Unreviewed)
    );
    assert_eq!(
        store
            .curation_logs_in_range(
                project_id,
                CurationLogFilter::Unreviewed,
                ActivityTimeRange::since(25),
                100,
            )
            .await
            .expect("bounded curation logs")
            .into_iter()
            .map(|log| log.id)
            .collect::<Vec<_>>(),
        vec![excluded, third],
        "curation uses the same captured-time boundary as the canvas and counts"
    );

    store
        .set_activity_excluded(excluded, true)
        .await
        .expect("exclude noise");
    assert_eq!(
        store
            .curation_logs(project_id, CurationLogFilter::Excluded, 100)
            .await
            .expect("excluded logs")[0]
            .id,
        excluded
    );

    let preparation = store
        .prepare_curation(project_id, &[third, first, second])
        .await
        .expect("prepare");
    assert!(preparation.cached().is_none());
    assert_eq!(
        preparation.selected_activity_ids(),
        &[first, second, third],
        "fingerprints and validation use canonical log order"
    );
    assert_eq!(
        preparation.input().logs[0].session_group,
        preparation.input().logs[1].session_group,
        "session continuity remains a compact weak signal"
    );
    assert_ne!(
        preparation.input().logs[1].session_group,
        preparation.input().logs[2].session_group
    );
    assert!(
        store
            .work_items(Some(project_id))
            .await
            .expect("works")
            .is_empty()
    );

    let groups = vec![
        group(None, "Windows Portable 배포", vec![first, second], 91),
        group(None, "Portable 용량 최적화", vec![third], 84),
    ];
    let proposal = store
        .save_curation_proposal(&preparation, groups.clone())
        .await
        .expect("proposal");
    assert!(!proposal.cached, "first proposal is newly persisted");
    let cached = store
        .prepare_curation(project_id, &[first, second, third])
        .await
        .expect("cached preparation");
    assert_eq!(cached.cached().expect("cache hit").id, proposal.id);
    assert!(
        store
            .work_items(Some(project_id))
            .await
            .expect("works")
            .is_empty()
    );

    let applied = store
        .apply_curation_proposal(proposal.id, groups)
        .await
        .expect("apply after confirmation");
    assert_eq!(applied.work_ids.len(), 2);
    let works = store.work_items(Some(project_id)).await.expect("work list");
    assert_eq!(works.len(), 2);
    assert_eq!(works.iter().map(|work| work.log_count).sum::<i64>(), 3);
    assert!(works.iter().all(|work| !work.preview_logs.is_empty()));
    let organized = store
        .curation_logs(project_id, CurationLogFilter::Organized, 100)
        .await
        .expect("organized logs");
    assert_eq!(organized.len(), 3);

    let source = works[0].id;
    let target = works[1].id;
    store
        .create_work_edge(source, target)
        .await
        .expect("user edge");
    assert_eq!(
        store
            .work_edges(Some(project_id))
            .await
            .expect("edges")
            .len(),
        1
    );
    let edge_id = store.work_edges(Some(project_id)).await.expect("edges")[0].id;
    store
        .delete_work_edge(edge_id)
        .await
        .expect("delete edge only");
    assert_eq!(
        store
            .work_items(Some(project_id))
            .await
            .expect("works")
            .len(),
        2
    );

    let detail = store.work_item(source).await.expect("work detail");
    let removed_log = detail.logs[0].id;
    store
        .remove_work_log(source, removed_log)
        .await
        .expect("return log to review queue");
    assert!(
        store
            .curation_logs(project_id, CurationLogFilter::Unreviewed, 100)
            .await
            .expect("unreviewed")
            .iter()
            .any(|log| log.id == removed_log)
    );

    store
        .soft_delete_activity(removed_log)
        .await
        .expect("soft delete");
    assert!(
        store
            .curation_logs(project_id, CurationLogFilter::All, 100)
            .await
            .expect("visible logs")
            .iter()
            .all(|log| log.id != removed_log),
        "soft-deleted evidence is hidden without deleting the database row"
    );
    assert!(matches!(
        store.set_activity_excluded(removed_log, true).await,
        Err(StoreError::ActivityNotFound(id)) if id == removed_log
    ));
}

#[tokio::test]
async fn proposals_reject_cross_project_duplicate_and_already_organized_logs() {
    let directory = TempDir::new().expect("directory");
    let first_cwd = directory.path().join("first");
    let second_cwd = directory.path().join("second");
    fs::create_dir(&first_cwd).expect("first directory");
    fs::create_dir(&second_cwd).expect("second directory");
    let store = ActivityStore::in_memory().await.expect("store");
    store.migrate().await.expect("migration");
    let first = record(&store, &first_cwd, "session", "one", "first", 1).await;
    let second = record(&store, &second_cwd, "session", "two", "second", 2).await;
    let first_project = project_id(&store, first).await;

    assert!(matches!(
        store
            .prepare_curation(first_project, &[first, second])
            .await,
        Err(StoreError::InvalidCuration(_))
    ));
    let preparation = store
        .prepare_curation(first_project, &[first])
        .await
        .expect("valid preparation");
    assert!(matches!(
        store
            .save_curation_proposal(
                &preparation,
                vec![group(None, "Duplicate", vec![first, first], 50)],
            )
            .await,
        Err(StoreError::InvalidCuration(_))
    ));
    let proposal = store
        .save_curation_proposal(
            &preparation,
            vec![group(None, "First work", vec![first], 90)],
        )
        .await
        .expect("proposal");
    store
        .apply_curation_proposal(
            proposal.id,
            vec![group(None, "First work", vec![first], 90)],
        )
        .await
        .expect("apply");
    assert!(matches!(
        store.prepare_curation(first_project, &[first]).await,
        Err(StoreError::InvalidCuration(_))
    ));
}

fn group(
    target_work_id: Option<i64>,
    title: &str,
    log_ids: Vec<i64>,
    confidence: u8,
) -> CurationProposalGroup {
    CurationProposalGroup {
        target_work_id,
        title: title.into(),
        log_ids,
        confidence,
        uncertain: confidence < 70,
    }
}

async fn record(
    store: &ActivityStore,
    cwd: &Path,
    session: &str,
    turn: &str,
    prompt: &str,
    captured_at_us: i64,
) -> i64 {
    let event = IngressEvent::try_new("codex", session, turn, cwd.to_string_lossy(), prompt, None)
        .expect("event");
    let origin = ProjectIdentity::capture_snapshot_from_cwd(cwd)
        .expect("origin")
        .origin;
    store
        .record(RecordActivity::captured(event, origin, captured_at_us))
        .await
        .expect("record")
}

async fn project_id(store: &ActivityStore, activity_id: i64) -> i64 {
    store
        .activity_detail(activity_id)
        .await
        .expect("detail")
        .project
        .expect("project")
        .id
}
