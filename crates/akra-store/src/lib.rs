#![forbid(unsafe_code)]

use std::path::Path;

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;

mod activities;
mod activity_assignments;
mod activity_details;
mod canvas;
mod capture_sources;
mod ingest;
mod migration;
mod migration_v2;
mod migration_v3;
mod migration_v4;
mod migration_v5;
mod migration_v6;
mod migration_v7;
mod models;
mod origin_transitions;
mod origins;
mod project_names;
mod projects;
mod providers;
mod routing;

pub use activities::{ActivityOrder, ActivityScope};
pub use activity_assignments::{
    ActivityAssignmentCommand, ActivityAssignmentResult, AssignmentDestination, FutureRouteAction,
    MAX_ACTIVITY_ASSIGNMENT_BATCH,
};
pub use capture_sources::CaptureClientObservation;
pub use ingest::RecordActivity;
pub use models::{
    ActivityConversationTurn, ActivityDetail, ActivityOriginDetail, ActivityProjectSummary,
    ActivitySummary, ActivityTechnicalDetail, ActivityTimeProvenance, ActivityTimeSummary,
    CanvasEdgeSummary, CanvasNodeSummary, OriginSummary, ProjectSummary, ProviderIntegration,
};
pub use origin_transitions::{OriginRoutingCommand, OriginRoutingMode, ProjectDestination};
pub use project_names::{ProjectName, ProjectNameError, ProjectNames};

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
    #[error("invalid origin transition: {0}")]
    InvalidOriginTransition(String),
    #[error("invalid activity assignment: {0}")]
    InvalidActivityAssignment(String),
    #[error("a project cannot be merged into itself")]
    SameProjectMerge,
    #[error("store invariant violated: {0}")]
    Invariant(String),
}
