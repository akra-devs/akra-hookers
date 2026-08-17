use std::fmt;

use akra_core::ingress::ResultEvent;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;

use crate::{ActivityStore, StoreError};

pub const RESULT_SUMMARY_MODEL: &str = "gpt-5.3-codex-spark";
pub const MAX_RESULT_SUMMARY_ATTEMPTS: i64 = 3;
pub const MAX_RESULT_SOURCE_RETENTION_US: i64 = 24 * 60 * 60 * 1_000_000;
pub const MAX_RESULT_SUMMARY_CHARS: usize = 180;
const MAX_STORED_ERROR_CHARS: usize = 2_048;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordResult {
    event: ResultEvent,
    captured_at_us: i64,
    source: Option<CaptureSource>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
struct CaptureSource {
    target: String,
    client: String,
}

impl RecordResult {
    pub const fn captured(event: ResultEvent, captured_at_us: i64) -> Self {
        Self {
            event,
            captured_at_us,
            source: None,
        }
    }

    pub fn captured_from(
        event: ResultEvent,
        captured_at_us: i64,
        target: impl Into<String>,
        client: impl Into<String>,
    ) -> Self {
        Self {
            event,
            captured_at_us,
            source: Some(CaptureSource {
                target: target.into(),
                client: client.into(),
            }),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultCaptureOutcome {
    Inserted,
    Updated,
    Duplicate,
    IgnoredStale,
    IgnoredEmpty,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultSummaryState {
    Pending,
    Running,
    RetryWait,
    Succeeded,
    Failed,
    Skipped,
}

impl ResultSummaryState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Skipped => "skipped",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "retry_wait" => Ok(Self::RetryWait),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "skipped" => Ok(Self::Skipped),
            _ => Err(StoreError::Invariant(format!(
                "invalid result summary state: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ResultSummaryLines {
    lines: [String; 3],
}

impl ResultSummaryLines {
    pub fn try_new(
        line_1: impl Into<String>,
        line_2: impl Into<String>,
        line_3: impl Into<String>,
    ) -> Result<Self, ResultSummaryValidationError> {
        let lines = normalize_summary_lines([line_1.into(), line_2.into(), line_3.into()])?;
        let character_count = summary_character_count(&lines);
        if character_count > MAX_RESULT_SUMMARY_CHARS {
            return Err(ResultSummaryValidationError::SummaryTooLong(
                character_count,
            ));
        }
        Ok(Self { lines })
    }

    pub(crate) fn compact_legacy(
        line_1: impl Into<String>,
        line_2: impl Into<String>,
        line_3: impl Into<String>,
    ) -> Result<Self, ResultSummaryValidationError> {
        let mut lines = normalize_summary_lines([line_1.into(), line_2.into(), line_3.into()])?;
        let lengths: [usize; 3] = std::array::from_fn(|index| lines[index].chars().count());
        if lengths.iter().sum::<usize>() > MAX_RESULT_SUMMARY_CHARS {
            let mut allocations = [1_usize; 3];
            let mut remaining = MAX_RESULT_SUMMARY_CHARS - allocations.len();
            while remaining > 0 {
                let mut allocated = false;
                for index in 0..allocations.len() {
                    if remaining == 0 {
                        break;
                    }
                    if allocations[index] < lengths[index] {
                        allocations[index] += 1;
                        remaining -= 1;
                        allocated = true;
                    }
                }
                if !allocated {
                    break;
                }
            }
            for index in 0..lines.len() {
                if allocations[index] < lengths[index] {
                    lines[index] = truncate_with_ellipsis(&lines[index], allocations[index]);
                }
            }
        }
        Self::try_new(lines[0].clone(), lines[1].clone(), lines[2].clone())
    }

    pub fn as_array(&self) -> &[String; 3] {
        &self.lines
    }

    pub fn into_array(self) -> [String; 3] {
        self.lines
    }
}

fn normalize_summary_lines(
    mut lines: [String; 3],
) -> Result<[String; 3], ResultSummaryValidationError> {
    for (index, line) in lines.iter_mut().enumerate() {
        *line = line.trim().to_owned();
        if line.is_empty() {
            return Err(ResultSummaryValidationError::BlankLine(index + 1));
        }
        if line.contains(['\r', '\n']) {
            return Err(ResultSummaryValidationError::EmbeddedNewline(index + 1));
        }
    }
    Ok(lines)
}

fn summary_character_count(lines: &[String; 3]) -> usize {
    lines.iter().map(|line| line.chars().count()).sum()
}

fn truncate_with_ellipsis(value: &str, maximum_characters: usize) -> String {
    if value.chars().count() <= maximum_characters {
        return value.to_owned();
    }
    if maximum_characters == 1 {
        return "…".to_owned();
    }
    value
        .chars()
        .take(maximum_characters - 1)
        .chain(std::iter::once('…'))
        .collect()
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum ResultSummaryValidationError {
    #[error("summary line {0} must not be blank")]
    BlankLine(usize),
    #[error("summary line {0} must not contain a newline")]
    EmbeddedNewline(usize),
    #[error(
        "summary must be at most {MAX_RESULT_SUMMARY_CHARS} characters across all three lines; got {0}"
    )]
    SummaryTooLong(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResultSummary {
    pub state: ResultSummaryState,
    pub lines: Option<ResultSummaryLines>,
    pub summary_model: String,
    pub attempt_count: i64,
    pub generation: i64,
    pub updated_at_us: i64,
    pub completed_at_us: Option<i64>,
    /// Whether transient raw assistant output is still retained for a future attempt.
    pub source_retained: bool,
}

#[derive(Clone)]
pub struct ResultSummaryClaim {
    provider: String,
    provider_session_id: String,
    provider_turn_id: String,
    activity_event_id: i64,
    source_text: String,
    generation: i64,
    lease_token: String,
    attempt_number: i64,
    summary_model: String,
    capture_target: Option<String>,
    capture_client: Option<String>,
}

impl fmt::Debug for ResultSummaryClaim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ResultSummaryClaim")
            .field("provider", &self.provider)
            .field("provider_session_id", &self.provider_session_id)
            .field("provider_turn_id", &self.provider_turn_id)
            .field("activity_event_id", &self.activity_event_id)
            .field("source_text_bytes", &self.source_text.len())
            .field("generation", &self.generation)
            .field("attempt_number", &self.attempt_number)
            .field("summary_model", &self.summary_model)
            .field("capture_target", &self.capture_target)
            .field("capture_client", &self.capture_client)
            .finish_non_exhaustive()
    }
}

impl ResultSummaryClaim {
    pub const fn activity_event_id(&self) -> i64 {
        self.activity_event_id
    }

    pub fn source_text(&self) -> &str {
        &self.source_text
    }

    pub const fn attempt_number(&self) -> i64 {
        self.attempt_number
    }

    pub fn summary_model(&self) -> &str {
        &self.summary_model
    }

    pub fn capture_target(&self) -> Option<&str> {
        self.capture_target.as_deref()
    }

    pub fn capture_client(&self) -> Option<&str> {
        self.capture_client.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultSummaryFailureDisposition {
    RetryScheduled,
    Failed,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ResultSummaryRegenerationOutcome {
    Scheduled,
    AlreadyPending,
    Unavailable,
}

impl ActivityStore {
    pub async fn capture_result(
        &self,
        command: RecordResult,
    ) -> Result<ResultCaptureOutcome, StoreError> {
        let event = &command.event;
        let Some(source_text) = event
            .result()
            .map(str::trim)
            .filter(|result| !result.is_empty())
            .map(ToOwned::to_owned)
        else {
            return Ok(ResultCaptureOutcome::IgnoredEmpty);
        };
        let source_digest = format!("{:x}", Sha256::digest(source_text.as_bytes()));
        let mut transaction = self.pool.begin().await?;
        let linked_activity = linked_activity(
            &mut transaction,
            event.provider().as_str(),
            event.session_id(),
            event.turn_id(),
        )
        .await?;
        let linked_activity_id = linked_activity
            .as_ref()
            .map(|(activity_event_id, _)| *activity_event_id);
        let eligible = linked_activity
            .as_ref()
            .is_none_or(|(_, activity_kind)| activity_kind == "user");
        let existing = sqlx::query(
            "SELECT source_digest, generation, captured_at_us
             FROM activity_result_summaries
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .fetch_optional(&mut *transaction)
        .await?;

        if let Some(existing) = existing.as_ref()
            && existing.try_get::<String, _>("source_digest")? == source_digest
        {
            if let Some((activity_event_id, _)) = linked_activity.as_ref() {
                link_activity(
                    &mut transaction,
                    *activity_event_id,
                    event.provider().as_str(),
                    event.session_id(),
                    event.turn_id(),
                )
                .await?;
            }
            transaction.commit().await?;
            return Ok(ResultCaptureOutcome::Duplicate);
        }

        if let Some(existing) = existing.as_ref()
            && command.captured_at_us <= existing.try_get::<i64, _>("captured_at_us")?
        {
            if let Some((activity_event_id, _)) = linked_activity.as_ref() {
                link_activity(
                    &mut transaction,
                    *activity_event_id,
                    event.provider().as_str(),
                    event.session_id(),
                    event.turn_id(),
                )
                .await?;
            }
            transaction.commit().await?;
            return Ok(ResultCaptureOutcome::IgnoredStale);
        }

        let outcome = if existing.is_some() {
            ResultCaptureOutcome::Updated
        } else {
            ResultCaptureOutcome::Inserted
        };
        let generation = existing
            .as_ref()
            .map(|row| row.try_get::<i64, _>("generation"))
            .transpose()?
            .unwrap_or(0)
            + 1;
        let state = if eligible { "pending" } else { "skipped" };
        let retained_source = eligible.then_some(source_text.as_str());
        let completed_at_us = (!eligible).then_some(command.captured_at_us);
        let (capture_target, capture_client) =
            command.source.as_ref().map_or((None, None), |source| {
                (Some(source.target.as_str()), Some(source.client.as_str()))
            });
        sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id, activity_event_id,
                 source_digest, source_text, state, generation, attempt_count,
                 next_attempt_at_us, lease_token, lease_expires_at_us, summary_model,
                 summary_line_1, summary_line_2, summary_line_3, last_error,
                 capture_target, capture_client, captured_at_us, updated_at_us,
                 completed_at_us
             ) VALUES (
                 ?, ?, ?, ?, ?, ?, ?, ?, 0, 0, NULL, NULL, ?,
                 NULL, NULL, NULL, NULL, ?, ?, ?, ?, ?
             )
             ON CONFLICT(provider, provider_session_id, provider_turn_id) DO UPDATE SET
                 activity_event_id = excluded.activity_event_id,
                 source_digest = excluded.source_digest,
                 source_text = excluded.source_text,
                 state = excluded.state,
                 generation = excluded.generation,
                 attempt_count = 0,
                 next_attempt_at_us = 0,
                 lease_token = NULL,
                 lease_expires_at_us = NULL,
                 summary_model = excluded.summary_model,
                 summary_line_1 = NULL,
                 summary_line_2 = NULL,
                 summary_line_3 = NULL,
                 last_error = NULL,
                 capture_target = excluded.capture_target,
                 capture_client = excluded.capture_client,
                 captured_at_us = excluded.captured_at_us,
                 updated_at_us = excluded.updated_at_us,
                 completed_at_us = excluded.completed_at_us",
        )
        .bind(event.provider().as_str())
        .bind(event.session_id())
        .bind(event.turn_id())
        .bind(linked_activity_id)
        .bind(source_digest)
        .bind(retained_source)
        .bind(state)
        .bind(generation)
        .bind(RESULT_SUMMARY_MODEL)
        .bind(capture_target)
        .bind(capture_client)
        .bind(command.captured_at_us)
        .bind(command.captured_at_us)
        .bind(completed_at_us)
        .execute(&mut *transaction)
        .await?;
        if let Some(activity_event_id) = linked_activity_id {
            crate::prompt_summaries::reconcile_context_dependents(
                &mut transaction,
                activity_event_id,
                command.captured_at_us,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(outcome)
    }

    pub async fn claim_result_summary(
        &self,
        now_us: i64,
        lease_duration_us: i64,
    ) -> Result<Option<ResultSummaryClaim>, StoreError> {
        let lease_expires_at_us = now_us
            .checked_add(lease_duration_us)
            .filter(|_| lease_duration_us > 0)
            .ok_or(StoreError::InvalidResultSummaryLease)?;
        let mut transaction = self.pool.begin().await?;

        let retention_cutoff_us = now_us.saturating_sub(MAX_RESULT_SOURCE_RETENTION_US);
        sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'failed', source_text = NULL, lease_token = NULL,
                 lease_expires_at_us = NULL,
                 last_error = 'result expired before summary completion',
                 updated_at_us = ?, completed_at_us = ?
             WHERE source_text IS NOT NULL AND captured_at_us <= ?
               AND (
                   state IN ('pending', 'retry_wait', 'failed')
                   OR (state = 'running' AND lease_expires_at_us <= ?)
               )",
        )
        .bind(now_us)
        .bind(now_us)
        .bind(retention_cutoff_us)
        .bind(now_us)
        .execute(&mut *transaction)
        .await?;

        // A worker that repeatedly dies cannot retain raw assistant output forever.
        sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'failed', source_text = NULL, lease_token = NULL,
                 lease_expires_at_us = NULL,
                 last_error = 'summary worker lease expired after maximum attempts',
                 updated_at_us = ?, completed_at_us = ?
             WHERE state = 'running' AND lease_expires_at_us <= ?
               AND attempt_count >= ?",
        )
        .bind(now_us)
        .bind(now_us)
        .bind(now_us)
        .bind(MAX_RESULT_SUMMARY_ATTEMPTS)
        .execute(&mut *transaction)
        .await?;

        let candidate = sqlx::query(
            "SELECT summaries.provider, summaries.provider_session_id,
                    summaries.provider_turn_id, summaries.activity_event_id,
                    summaries.source_text, summaries.generation,
                    summaries.attempt_count, summaries.summary_model,
                    summaries.capture_target, summaries.capture_client
             FROM activity_result_summaries AS summaries
             JOIN activity_events AS activities
               ON activities.id = summaries.activity_event_id
              WHERE activities.activity_kind = 'user'
                AND activities.deleted_at_us IS NULL
               AND summaries.source_text IS NOT NULL
               AND summaries.attempt_count < ?
               AND (
                   (summaries.state IN ('pending', 'retry_wait')
                       AND summaries.next_attempt_at_us <= ?)
                   OR
                   (summaries.state = 'running'
                       AND summaries.lease_expires_at_us <= ?)
               )
             ORDER BY summaries.captured_at_us,
                      summaries.provider, summaries.provider_session_id,
                      summaries.provider_turn_id
             LIMIT 1",
        )
        .bind(MAX_RESULT_SUMMARY_ATTEMPTS)
        .bind(now_us)
        .bind(now_us)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(candidate) = candidate else {
            transaction.commit().await?;
            return Ok(None);
        };
        let provider: String = candidate.try_get("provider")?;
        let provider_session_id: String = candidate.try_get("provider_session_id")?;
        let provider_turn_id: String = candidate.try_get("provider_turn_id")?;
        let generation: i64 = candidate.try_get("generation")?;
        let lease_token: String = sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
            .fetch_one(&mut *transaction)
            .await?;
        let updated = sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'running', attempt_count = attempt_count + 1,
                 lease_token = ?, lease_expires_at_us = ?, updated_at_us = ?
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?
               AND generation = ?
               AND (
                   (state IN ('pending', 'retry_wait') AND next_attempt_at_us <= ?)
                   OR (state = 'running' AND lease_expires_at_us <= ?)
               )",
        )
        .bind(&lease_token)
        .bind(lease_expires_at_us)
        .bind(now_us)
        .bind(&provider)
        .bind(&provider_session_id)
        .bind(&provider_turn_id)
        .bind(generation)
        .bind(now_us)
        .bind(now_us)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            transaction.commit().await?;
            return Ok(None);
        }
        transaction.commit().await?;
        Ok(Some(ResultSummaryClaim {
            provider,
            provider_session_id,
            provider_turn_id,
            activity_event_id: candidate.try_get("activity_event_id")?,
            source_text: candidate.try_get("source_text")?,
            generation,
            lease_token,
            attempt_number: candidate.try_get::<i64, _>("attempt_count")? + 1,
            summary_model: candidate.try_get("summary_model")?,
            capture_target: candidate.try_get("capture_target")?,
            capture_client: candidate.try_get("capture_client")?,
        }))
    }

    pub async fn complete_result_summary(
        &self,
        claim: &ResultSummaryClaim,
        lines: &ResultSummaryLines,
        completed_at_us: i64,
    ) -> Result<bool, StoreError> {
        let lines = lines.as_array();
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'succeeded', source_text = NULL,
                 lease_token = NULL, lease_expires_at_us = NULL,
                 summary_line_1 = ?, summary_line_2 = ?, summary_line_3 = ?,
                 last_error = NULL, updated_at_us = ?, completed_at_us = ?
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?
               AND generation = ? AND state = 'running' AND lease_token = ?",
        )
        .bind(&lines[0])
        .bind(&lines[1])
        .bind(&lines[2])
        .bind(completed_at_us)
        .bind(completed_at_us)
        .bind(&claim.provider)
        .bind(&claim.provider_session_id)
        .bind(&claim.provider_turn_id)
        .bind(claim.generation)
        .bind(&claim.lease_token)
        .execute(&mut *transaction)
        .await?;
        let applied = updated.rows_affected() == 1;
        if applied {
            crate::prompt_summaries::reconcile_context_dependents(
                &mut transaction,
                claim.activity_event_id,
                completed_at_us,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(applied)
    }

    pub async fn fail_result_summary(
        &self,
        claim: &ResultSummaryClaim,
        error: &str,
        retry_at_us: Option<i64>,
        failed_at_us: i64,
    ) -> Result<ResultSummaryFailureDisposition, StoreError> {
        let should_retry =
            retry_at_us.is_some() && claim.attempt_number < MAX_RESULT_SUMMARY_ATTEMPTS;
        let state = if should_retry { "retry_wait" } else { "failed" };
        // A terminal automatic failure remains manually retryable only inside
        // the same 24-hour raw-result retention window. The claim sweep below
        // remains authoritative and scrubs it once that window closes.
        let retained_source = Some(claim.source_text.as_str());
        let completed_at_us = (!should_retry).then_some(failed_at_us);
        let next_attempt_at_us = retry_at_us.filter(|_| should_retry).unwrap_or(0);
        let error = sanitize_error(error);
        let mut transaction = self.pool.begin().await?;
        let updated = sqlx::query(
            "UPDATE activity_result_summaries
             SET state = ?, source_text = ?, next_attempt_at_us = ?,
                 lease_token = NULL, lease_expires_at_us = NULL,
                 last_error = ?, updated_at_us = ?, completed_at_us = ?
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?
               AND generation = ? AND state = 'running' AND lease_token = ?",
        )
        .bind(state)
        .bind(retained_source)
        .bind(next_attempt_at_us)
        .bind(error)
        .bind(failed_at_us)
        .bind(completed_at_us)
        .bind(&claim.provider)
        .bind(&claim.provider_session_id)
        .bind(&claim.provider_turn_id)
        .bind(claim.generation)
        .bind(&claim.lease_token)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() == 0 {
            transaction.commit().await?;
            return Ok(ResultSummaryFailureDisposition::Stale);
        }
        if !should_retry {
            crate::prompt_summaries::reconcile_context_dependents(
                &mut transaction,
                claim.activity_event_id,
                failed_at_us,
            )
            .await?;
        }
        transaction.commit().await?;
        Ok(if should_retry {
            ResultSummaryFailureDisposition::RetryScheduled
        } else {
            ResultSummaryFailureDisposition::Failed
        })
    }

    pub async fn regenerate_result_summary(
        &self,
        activity_event_id: i64,
        requested_at_us: i64,
    ) -> Result<ResultSummaryRegenerationOutcome, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query(
            "SELECT summaries.state, summaries.source_text IS NOT NULL AS source_retained,
                    summaries.captured_at_us
             FROM activity_events AS activities
             LEFT JOIN activity_result_summaries AS summaries
               ON summaries.activity_event_id = activities.id
             WHERE activities.id = ? AND activities.activity_kind = 'user'
               AND activities.deleted_at_us IS NULL",
        )
        .bind(activity_event_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::ActivityNotFound(activity_event_id))?;
        let state: Option<String> = row.try_get("state")?;
        let source_retained: Option<i64> = row.try_get("source_retained")?;
        let captured_at_us: Option<i64> = row.try_get("captured_at_us")?;
        let source_retained = source_retained.is_some_and(|value| value != 0);

        if source_retained
            && state
                .as_deref()
                .is_some_and(|value| matches!(value, "pending" | "running" | "retry_wait"))
        {
            transaction.commit().await?;
            return Ok(ResultSummaryRegenerationOutcome::AlreadyPending);
        }

        let retention_cutoff_us = requested_at_us.saturating_sub(MAX_RESULT_SOURCE_RETENTION_US);
        if source_retained
            && captured_at_us.is_some_and(|captured_at_us| captured_at_us <= retention_cutoff_us)
        {
            sqlx::query(
                "UPDATE activity_result_summaries
                 SET source_text = NULL, lease_token = NULL, lease_expires_at_us = NULL,
                     last_error = 'result expired before manual regeneration',
                     updated_at_us = ?, completed_at_us = ?
                 WHERE activity_event_id = ?",
            )
            .bind(requested_at_us)
            .bind(requested_at_us)
            .bind(activity_event_id)
            .execute(&mut *transaction)
            .await?;
            transaction.commit().await?;
            return Ok(ResultSummaryRegenerationOutcome::Unavailable);
        }

        if state.as_deref() != Some("failed") || !source_retained {
            transaction.commit().await?;
            return Ok(ResultSummaryRegenerationOutcome::Unavailable);
        }

        let updated = sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'pending', generation = generation + 1, attempt_count = 0,
                 next_attempt_at_us = 0, lease_token = NULL, lease_expires_at_us = NULL,
                 summary_model = ?, summary_line_1 = NULL, summary_line_2 = NULL,
                 summary_line_3 = NULL, last_error = NULL, updated_at_us = ?,
                 completed_at_us = NULL
             WHERE activity_event_id = ? AND state = 'failed' AND source_text IS NOT NULL",
        )
        .bind(RESULT_SUMMARY_MODEL)
        .bind(requested_at_us)
        .bind(activity_event_id)
        .execute(&mut *transaction)
        .await?;
        transaction.commit().await?;
        Ok(if updated.rows_affected() == 1 {
            ResultSummaryRegenerationOutcome::Scheduled
        } else {
            ResultSummaryRegenerationOutcome::Unavailable
        })
    }

    pub async fn result_summary(
        &self,
        activity_event_id: i64,
    ) -> Result<Option<ResultSummary>, StoreError> {
        let row = sqlx::query(
            "SELECT state, summary_model, summary_line_1, summary_line_2,
                    summary_line_3, attempt_count, generation, updated_at_us,
                    completed_at_us, source_text IS NOT NULL AS source_retained
             FROM activity_result_summaries WHERE activity_event_id = ?",
        )
        .bind(activity_event_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|row| {
            let state = ResultSummaryState::from_storage(row.try_get("state")?)?;
            let line_1: Option<String> = row.try_get("summary_line_1")?;
            let line_2: Option<String> = row.try_get("summary_line_2")?;
            let line_3: Option<String> = row.try_get("summary_line_3")?;
            let lines = match (line_1, line_2, line_3) {
                (Some(line_1), Some(line_2), Some(line_3)) => {
                    Some(ResultSummaryLines::try_new(line_1, line_2, line_3)?)
                }
                (None, None, None) => None,
                _ => {
                    return Err(StoreError::Invariant(
                        "result summary contains a partial set of lines".to_owned(),
                    ));
                }
            };
            Ok(ResultSummary {
                state,
                lines,
                summary_model: row.try_get("summary_model")?,
                attempt_count: row.try_get("attempt_count")?,
                generation: row.try_get("generation")?,
                updated_at_us: row.try_get("updated_at_us")?,
                completed_at_us: row.try_get("completed_at_us")?,
                source_retained: row.try_get("source_retained")?,
            })
        })
        .transpose()
    }
}

pub(crate) async fn link_activity(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    provider: &str,
    provider_session_id: &str,
    provider_turn_id: &str,
) -> Result<(), StoreError> {
    let activity_kind: String =
        sqlx::query_scalar("SELECT activity_kind FROM activity_events WHERE id = ?")
            .bind(activity_event_id)
            .fetch_one(&mut **transaction)
            .await?;
    if activity_kind == "user" {
        sqlx::query(
            "UPDATE activity_result_summaries
             SET activity_event_id = ?
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(activity_event_id)
        .bind(provider)
        .bind(provider_session_id)
        .bind(provider_turn_id)
        .execute(&mut **transaction)
        .await?;
    } else {
        sqlx::query(
            "UPDATE activity_result_summaries
             SET activity_event_id = ?, state = 'skipped', source_text = NULL,
                 lease_token = NULL, lease_expires_at_us = NULL,
                 summary_line_1 = NULL, summary_line_2 = NULL, summary_line_3 = NULL,
                 next_attempt_at_us = 0, completed_at_us = updated_at_us
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(activity_event_id)
        .bind(provider)
        .bind(provider_session_id)
        .bind(provider_turn_id)
        .execute(&mut **transaction)
        .await?;
    }
    Ok(())
}

async fn linked_activity(
    transaction: &mut Transaction<'_, Sqlite>,
    provider: &str,
    provider_session_id: &str,
    provider_turn_id: &str,
) -> Result<Option<(i64, String)>, StoreError> {
    let row = sqlx::query(
        "SELECT activities.id, activities.activity_kind
         FROM ingest_dedupes AS dedupes
         JOIN activity_events AS activities ON activities.id = dedupes.activity_event_id
         WHERE dedupes.provider = ? AND dedupes.provider_session_id = ?
           AND dedupes.provider_turn_id = ?",
    )
    .bind(provider)
    .bind(provider_session_id)
    .bind(provider_turn_id)
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(|row| Ok((row.try_get("id")?, row.try_get("activity_kind")?)))
        .transpose()
}

fn sanitize_error(error: &str) -> String {
    error
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .take(MAX_STORED_ERROR_CHARS)
        .collect::<String>()
        .trim()
        .to_owned()
}
