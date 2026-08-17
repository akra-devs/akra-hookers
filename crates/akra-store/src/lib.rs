#![forbid(unsafe_code)]

use std::path::Path;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;

mod activities;
mod activity_assignments;
mod activity_deletions;
mod activity_details;
mod canvas;
mod capture_sources;
mod ingest;
mod migration;
mod migration_v10;
mod migration_v11;
mod migration_v12;
mod migration_v13;
mod migration_v2;
mod migration_v3;
mod migration_v4;
mod migration_v5;
mod migration_v6;
mod migration_v7;
mod migration_v8;
mod migration_v9;
mod models;
mod origin_transitions;
mod origins;
mod project_names;
mod projects;
mod prompt_summaries;
mod providers;
mod result_summaries;
mod routing;
mod work_curation;

pub use activities::{ActivityKindFilter, ActivityOrder, ActivityScope, ActivityTimeRange};
pub use activity_assignments::{
    ActivityAssignmentCommand, ActivityAssignmentResult, AssignmentDestination, FutureRouteAction,
    MAX_ACTIVITY_ASSIGNMENT_BATCH,
};
pub use capture_sources::CaptureClientObservation;
pub use ingest::RecordActivity;
pub use models::{
    ActivityConversationTurn, ActivityDetail, ActivityOriginDetail, ActivityProjectSummary,
    ActivityPromptSummary, ActivityResultSummary, ActivitySummary, ActivityTechnicalDetail,
    ActivityTimeProvenance, ActivityTimeSummary, CanvasEdgeSummary, CanvasNodeSummary,
    CurationApplyResult, CurationLogState, CurationLogSummary, CurationProposal,
    CurationProposalGroup, OriginSummary, ProjectSummary, PromptSummaryMode, PromptSummaryStatus,
    ProviderIntegration, ResultSummaryStatus, WorkEdgeSummary, WorkItemDetail, WorkItemSummary,
    WorkLogSummary,
};
pub use origin_transitions::{OriginRoutingCommand, OriginRoutingMode, ProjectDestination};
pub use project_names::{ProjectName, ProjectNameError, ProjectNames};
pub use prompt_summaries::{
    MAX_PROMPT_SUMMARY_ATTEMPTS, MAX_PROMPT_SUMMARY_CHARS, MAX_PROMPT_SUMMARY_INPUT_CHARS,
    PROMPT_SUMMARY_MODEL, PromptSummary, PromptSummaryClaim, PromptSummaryCompletionOutcome,
    PromptSummaryErrorCode, PromptSummaryFailureDisposition, PromptSummaryPolicy,
    PromptSummaryState, PromptSummaryText, PromptSummaryValidationError,
};
pub use result_summaries::{
    MAX_RESULT_SOURCE_RETENTION_US, MAX_RESULT_SUMMARY_ATTEMPTS, MAX_RESULT_SUMMARY_CHARS,
    RESULT_SUMMARY_MODEL, RecordResult, ResultCaptureOutcome, ResultSummary, ResultSummaryClaim,
    ResultSummaryFailureDisposition, ResultSummaryLines, ResultSummaryRegenerationOutcome,
    ResultSummaryState, ResultSummaryValidationError,
};
pub use work_curation::{
    CURATION_MODEL, CurationLogFilter, CurationModelInput, CurationModelLog, CurationModelWork,
    CurationPreparation, MAX_CURATION_CANDIDATES, MAX_CURATION_LOGS, MAX_WORK_TITLE_CHARS,
};

#[derive(Debug)]
pub struct ActivityStore {
    pool: SqlitePool,
}

impl ActivityStore {
    pub async fn open(path: &Path) -> Result<Self, StoreError> {
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .foreign_keys(true);
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect_with(options)
            .await?;
        Ok(Self { pool })
    }

    pub async fn in_memory() -> Result<Self, StoreError> {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await?;
        Ok(Self { pool })
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] sqlx::Error),
    #[error(transparent)]
    InvalidProjectName(#[from] ProjectNameError),
    #[error("project name already exists")]
    ProjectNameConflict,
    #[error("project not found: {0}")]
    ProjectNotFound(i64),
    #[error("origin not found: {0}")]
    OriginNotFound(i64),
    #[error("activity not found: {0}")]
    ActivityNotFound(i64),
    #[error("work not found: {0}")]
    WorkNotFound(i64),
    #[error("work edge not found: {0}")]
    WorkEdgeNotFound(i64),
    #[error("curation proposal not found: {0}")]
    CurationProposalNotFound(i64),
    #[error("invalid origin transition: {0}")]
    InvalidOriginTransition(String),
    #[error("invalid activity assignment: {0}")]
    InvalidActivityAssignment(String),
    #[error("invalid work curation: {0}")]
    InvalidCuration(String),
    #[error("a project cannot be merged into itself")]
    SameProjectMerge,
    #[error("store invariant violated: {0}")]
    Invariant(String),
    #[error("result summary lease duration must be positive and must not overflow")]
    InvalidResultSummaryLease,
    #[error("prompt summary lease duration must be positive and must not overflow")]
    InvalidPromptSummaryLease,
    #[error(transparent)]
    InvalidResultSummary(#[from] ResultSummaryValidationError),
    #[error(transparent)]
    InvalidPromptSummary(#[from] PromptSummaryValidationError),
}
