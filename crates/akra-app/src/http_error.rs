use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;

#[derive(Debug)]
pub(crate) struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

#[derive(Serialize)]
struct ErrorBody {
    code: &'static str,
    message: String,
}

impl ApiError {
    pub(crate) fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", message)
    }

    pub(crate) fn unprocessable(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, code, message)
    }

    pub(crate) fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal_error",
            "The request could not be completed.",
        )
    }

    pub(crate) fn from_store(error: akra_store::StoreError) -> Self {
        match error {
            akra_store::StoreError::InvalidProjectName(error) => {
                Self::unprocessable("invalid_project_name", error.to_string())
            }
            akra_store::StoreError::ProjectNameConflict => Self::new(
                StatusCode::CONFLICT,
                "project_name_conflict",
                "A project with that name already exists.",
            ),
            error @ (akra_store::StoreError::ProjectNotFound(_)
            | akra_store::StoreError::OriginNotFound(_)
            | akra_store::StoreError::ActivityNotFound(_)) => Self::not_found(error.to_string()),
            akra_store::StoreError::InvalidOriginTransition(message) => {
                Self::unprocessable("invalid_origin_transition", message)
            }
            akra_store::StoreError::InvalidActivityAssignment(message) => {
                Self::unprocessable("invalid_activity_assignment", message)
            }
            akra_store::StoreError::SameProjectMerge => {
                Self::unprocessable("same_project_merge", error.to_string())
            }
            akra_store::StoreError::Sqlite(_) | akra_store::StoreError::Invariant(_) => {
                Self::internal()
            }
        }
    }

    fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                code: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}
