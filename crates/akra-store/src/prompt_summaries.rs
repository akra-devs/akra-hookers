use std::fmt;

use akra_core::{
    ingress::ActivityKind,
    prompt_projection::{PromptProjection, PromptProjectionKind},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{Row, Sqlite, Transaction};
use thiserror::Error;

use crate::{
    ActivityPromptSummary, ActivityStore, PromptSummaryMode, PromptSummaryStatus, StoreError,
};

pub const PROMPT_SUMMARY_MODEL: &str = "gpt-5.3-codex-spark";
pub const MAX_PROMPT_SUMMARY_CHARS: usize = 96;
pub const MAX_PROMPT_SUMMARY_ATTEMPTS: i64 = 2;
pub const MAX_PROMPT_SUMMARY_INPUT_CHARS: usize = 8_000;
const PROMPT_SUMMARY_VERSION_PREFIX: &str = "prompt-summary-v1";

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSummaryPolicy {
    #[default]
    Off,
    Smart,
}

impl PromptSummaryPolicy {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Off => "off",
            Self::Smart => "smart",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "off" => Ok(Self::Off),
            "smart" => Ok(Self::Smart),
            _ => Err(StoreError::Invariant(format!(
                "invalid prompt summary policy: {value}"
            ))),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptSummaryState {
    Passthrough,
    WaitingContext,
    Pending,
    Running,
    RetryWait,
    Succeeded,
    Failed,
}

impl PromptSummaryState {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Passthrough => "passthrough",
            Self::WaitingContext => "waiting_context",
            Self::Pending => "pending",
            Self::Running => "running",
            Self::RetryWait => "retry_wait",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
        }
    }

    fn from_storage(value: &str) -> Result<Self, StoreError> {
        match value {
            "passthrough" => Ok(Self::Passthrough),
            "waiting_context" => Ok(Self::WaitingContext),
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "retry_wait" => Ok(Self::RetryWait),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            _ => Err(StoreError::Invariant(format!(
                "invalid prompt summary state: {value}"
            ))),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct PromptSummaryText(String);

impl PromptSummaryText {
    pub fn try_new(value: impl Into<String>) -> Result<Self, PromptSummaryValidationError> {
        let value = value.into();
        if value.trim().is_empty() {
            return Err(PromptSummaryValidationError::Blank);
        }
        if value.trim() != value {
            return Err(PromptSummaryValidationError::SurroundingWhitespace);
        }
        if value.contains(['\r', '\n']) {
            return Err(PromptSummaryValidationError::EmbeddedNewline);
        }
        if value.chars().any(char::is_control) {
            return Err(PromptSummaryValidationError::ControlCharacter);
        }
        if starts_markdown(&value) {
            return Err(PromptSummaryValidationError::MarkdownPrefix);
        }
        if !value.chars().any(is_hangul_syllable) {
            return Err(PromptSummaryValidationError::NonKorean);
        }
        if contains_multiple_sentences(&value) {
            return Err(PromptSummaryValidationError::MultipleSentences);
        }
        let character_count = value.chars().count();
        if character_count > MAX_PROMPT_SUMMARY_CHARS {
            return Err(PromptSummaryValidationError::SummaryTooLong(
                character_count,
            ));
        }
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn into_inner(self) -> String {
        self.0
    }
}

fn is_hangul_syllable(character: char) -> bool {
    matches!(character, '\u{AC00}'..='\u{D7A3}')
}

fn starts_markdown(value: &str) -> bool {
    let trimmed = value.trim_start();
    trimmed.starts_with('#')
        || trimmed.starts_with("```")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.split_once('.').is_some_and(|(prefix, rest)| {
            !prefix.is_empty() && prefix.chars().all(char::is_numeric) && rest.starts_with(' ')
        })
}

/// Prompt summaries intentionally contain one compact sentence. A terminator
/// followed by more non-closing content introduces another sentence. Decimals,
/// URLs, and a final terminator remain valid because their punctuation is not
/// followed by a sentence boundary.
fn contains_multiple_sentences(value: &str) -> bool {
    let characters = value.chars().collect::<Vec<_>>();
    for (index, character) in characters.iter().copied().enumerate() {
        if !matches!(character, '.' | '!' | '?' | '。' | '！' | '？') {
            continue;
        }
        let mut remainder = characters[index + 1..].iter().copied().peekable();
        let Some(next) = remainder.peek().copied() else {
            continue;
        };
        if !next.is_whitespace() && !matches!(next, '"' | '\'' | ')' | ']' | '}' | '”' | '’') {
            continue;
        }
        while remainder.peek().is_some_and(|next| {
            next.is_whitespace() || matches!(*next, '"' | '\'' | ')' | ']' | '}' | '”' | '’')
        }) {
            remainder.next();
        }
        if remainder.peek().is_some() {
            return true;
        }
    }
    false
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum PromptSummaryValidationError {
    #[error("prompt summary must not be blank")]
    Blank,
    #[error("prompt summary must not have leading or trailing whitespace")]
    SurroundingWhitespace,
    #[error("prompt summary must not contain a newline")]
    EmbeddedNewline,
    #[error("prompt summary must not contain control characters")]
    ControlCharacter,
    #[error("prompt summary must be written in Korean")]
    NonKorean,
    #[error("prompt summary must not start with Markdown syntax")]
    MarkdownPrefix,
    #[error("prompt summary must contain one sentence")]
    MultipleSentences,
    #[error("prompt summary must be at most {MAX_PROMPT_SUMMARY_CHARS} characters; got {0}")]
    SummaryTooLong(usize),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PromptSummaryErrorCode {
    InvalidOutput,
    OutputTooLong(usize),
    Runtime,
    Timeout,
    UnexpectedModel,
}

impl PromptSummaryErrorCode {
    fn storage_code(&self) -> String {
        match self {
            Self::InvalidOutput => "invalid_output".to_owned(),
            Self::OutputTooLong(characters) => format!("output_too_long:{characters}"),
            Self::Runtime => "runtime".to_owned(),
            Self::Timeout => "timeout".to_owned(),
            Self::UnexpectedModel => "unexpected_model".to_owned(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PromptSummary {
    pub state: PromptSummaryState,
    pub projection_kind: PromptProjectionKind,
    pub projected_prompt: String,
    pub text: Option<PromptSummaryText>,
    pub used_previous_result: bool,
    pub context_activity_event_id: Option<i64>,
    pub context_result_generation: Option<i64>,
    pub source_digest: String,
    pub generation: i64,
    pub attempt_count: i64,
    pub summary_model: String,
    pub updated_at_us: i64,
}

#[derive(Clone)]
pub struct PromptSummaryClaim {
    activity_event_id: i64,
    projected_prompt: String,
    previous_result_lines: Option<[String; 3]>,
    generation: i64,
    lease_token: String,
    attempt_number: i64,
    summary_model: String,
    capture_target: Option<String>,
    capture_client: Option<String>,
    previous_failure_code: Option<String>,
}

impl fmt::Debug for PromptSummaryClaim {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PromptSummaryClaim")
            .field("activity_event_id", &self.activity_event_id)
            .field(
                "projected_prompt_chars",
                &self.projected_prompt.chars().count(),
            )
            .field(
                "uses_previous_result",
                &self.previous_result_lines.is_some(),
            )
            .field("generation", &self.generation)
            .field("attempt_number", &self.attempt_number)
            .field("summary_model", &self.summary_model)
            .field("capture_target", &self.capture_target)
            .field("capture_client", &self.capture_client)
            .field("previous_failure_code", &self.previous_failure_code)
            .finish_non_exhaustive()
    }
}

impl PromptSummaryClaim {
    pub const fn activity_event_id(&self) -> i64 {
        self.activity_event_id
    }

    pub fn projected_prompt(&self) -> &str {
        &self.projected_prompt
    }

    pub fn previous_result_lines(&self) -> Option<&[String; 3]> {
        self.previous_result_lines.as_ref()
    }

    pub const fn generation(&self) -> i64 {
        self.generation
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

    pub fn previous_failure_code(&self) -> Option<&str> {
        self.previous_failure_code.as_deref()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptSummaryCompletionOutcome {
    Applied,
    Stale,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PromptSummaryFailureDisposition {
    RetryScheduled,
    Failed,
    Stale,
}

#[derive(Clone)]
struct ActivityInput {
    id: i64,
    provider: String,
    prompt: String,
    activity_kind: String,
}

#[derive(Clone)]
struct PreviousContext {
    activity_event_id: i64,
    result: PreviousResult,
}

#[derive(Clone)]
enum PreviousResult {
    Ready { generation: i64, lines: [String; 3] },
    Pending,
    Missing,
    TerminalUnavailable,
}

#[derive(Clone)]
struct DesiredSummary {
    state: PromptSummaryState,
    text: Option<String>,
    used_previous_result: bool,
    context_activity_event_id: Option<i64>,
    context_result_generation: Option<i64>,
    source_digest: String,
}

#[derive(Clone, Copy)]
struct SummaryGate {
    needs_summary: bool,
    needs_previous_result: bool,
}

impl ActivityStore {
    pub async fn prompt_summary_policy(
        &self,
        provider: &str,
    ) -> Result<PromptSummaryPolicy, StoreError> {
        let value: Option<String> = sqlx::query_scalar(
            "SELECT prompt_summary_mode FROM provider_summary_settings WHERE provider = ?",
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?;
        value
            .as_deref()
            .map(PromptSummaryPolicy::from_storage)
            .transpose()
            .map(|policy| policy.unwrap_or_default())
    }

    pub async fn set_prompt_summary_policy(
        &self,
        provider: &str,
        policy: PromptSummaryPolicy,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO provider_summary_settings (provider, prompt_summary_mode, updated_at_us)
             VALUES (?, ?, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))
             ON CONFLICT(provider) DO UPDATE SET
                 prompt_summary_mode = excluded.prompt_summary_mode,
                 updated_at_us = excluded.updated_at_us",
        )
        .bind(provider)
        .bind(policy.as_str())
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn initialize_prompt_summary(
        &self,
        activity_event_id: i64,
        projection: &PromptProjection,
        now_us: i64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        initialize_prompt_summary_in_transaction(
            &mut transaction,
            activity_event_id,
            projection,
            now_us,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn reconcile_prompt_summary_context(
        &self,
        activity_event_id: i64,
        now_us: i64,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        reconcile_prompt_summary_context_in_transaction(
            &mut transaction,
            activity_event_id,
            now_us,
        )
        .await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn claim_prompt_summary(
        &self,
        now_us: i64,
        lease_duration_us: i64,
    ) -> Result<Option<PromptSummaryClaim>, StoreError> {
        let lease_expires_at_us = now_us
            .checked_add(lease_duration_us)
            .filter(|_| lease_duration_us > 0)
            .ok_or(StoreError::InvalidPromptSummaryLease)?;
        let mut transaction = self.pool.begin().await?;
        sqlx::query(
            "UPDATE activity_prompt_summaries
             SET state = 'failed', lease_token = NULL, lease_expires_at_us = NULL,
                 last_error_code = 'lease_expired', updated_at_us = ?
             WHERE state = 'running' AND lease_expires_at_us <= ?
               AND attempt_count >= ?",
        )
        .bind(now_us)
        .bind(now_us)
        .bind(MAX_PROMPT_SUMMARY_ATTEMPTS)
        .execute(&mut *transaction)
        .await?;

        let candidate = sqlx::query(
            "SELECT summaries.activity_event_id,
                    COALESCE(summaries.projected_prompt, activities.prompt) AS projected_prompt,
                    summaries.used_previous_result, summaries.context_result_generation,
                    summaries.generation, summaries.attempt_count, summaries.summary_model,
                    summaries.last_error_code,
                    activities.capture_target, activities.capture_client,
                    context_summary.summary_line_1, context_summary.summary_line_2,
                    context_summary.summary_line_3
             FROM activity_prompt_summaries AS summaries
             JOIN activity_events AS activities ON activities.id = summaries.activity_event_id
             LEFT JOIN activity_result_summaries AS context_summary
               ON context_summary.activity_event_id = summaries.context_activity_event_id
              AND context_summary.generation = summaries.context_result_generation
              AND context_summary.state = 'succeeded'
              WHERE activities.activity_kind = 'user'
                AND activities.deleted_at_us IS NULL
               AND summaries.attempt_count < ?
               AND (
                   (summaries.state IN ('pending', 'retry_wait')
                     AND summaries.next_attempt_at_us <= ?)
                   OR (summaries.state = 'running' AND summaries.lease_expires_at_us <= ?)
               )
               AND (
                   summaries.used_previous_result = 0
                   OR context_summary.activity_event_id IS NOT NULL
               )
             ORDER BY summaries.updated_at_us, summaries.activity_event_id
             LIMIT 1",
        )
        .bind(MAX_PROMPT_SUMMARY_ATTEMPTS)
        .bind(now_us)
        .bind(now_us)
        .fetch_optional(&mut *transaction)
        .await?;
        let Some(candidate) = candidate else {
            transaction.commit().await?;
            return Ok(None);
        };
        let activity_event_id: i64 = candidate.try_get("activity_event_id")?;
        let generation: i64 = candidate.try_get("generation")?;
        let lease_token: String = sqlx::query_scalar("SELECT lower(hex(randomblob(16)))")
            .fetch_one(&mut *transaction)
            .await?;
        let updated = sqlx::query(
            "UPDATE activity_prompt_summaries
             SET state = 'running', attempt_count = attempt_count + 1,
                 lease_token = ?, lease_expires_at_us = ?, updated_at_us = ?
             WHERE activity_event_id = ? AND generation = ?
               AND (
                   (state IN ('pending', 'retry_wait') AND next_attempt_at_us <= ?)
                   OR (state = 'running' AND lease_expires_at_us <= ?)
               )",
        )
        .bind(&lease_token)
        .bind(lease_expires_at_us)
        .bind(now_us)
        .bind(activity_event_id)
        .bind(generation)
        .bind(now_us)
        .bind(now_us)
        .execute(&mut *transaction)
        .await?;
        if updated.rows_affected() != 1 {
            transaction.commit().await?;
            return Ok(None);
        }

        let previous_result_lines = match candidate.try_get::<i64, _>("used_previous_result")? {
            0 => None,
            1 => Some([
                candidate.try_get("summary_line_1")?,
                candidate.try_get("summary_line_2")?,
                candidate.try_get("summary_line_3")?,
            ]),
            value => {
                return Err(StoreError::Invariant(format!(
                    "invalid prompt summary previous-result marker: {value}"
                )));
            }
        };
        transaction.commit().await?;
        Ok(Some(PromptSummaryClaim {
            activity_event_id,
            projected_prompt: candidate.try_get("projected_prompt")?,
            previous_result_lines,
            generation,
            lease_token,
            attempt_number: candidate.try_get::<i64, _>("attempt_count")? + 1,
            summary_model: candidate.try_get("summary_model")?,
            capture_target: candidate.try_get("capture_target")?,
            capture_client: candidate.try_get("capture_client")?,
            previous_failure_code: candidate.try_get("last_error_code")?,
        }))
    }

    pub async fn complete_prompt_summary(
        &self,
        claim: &PromptSummaryClaim,
        text: &PromptSummaryText,
        now_us: i64,
    ) -> Result<PromptSummaryCompletionOutcome, StoreError> {
        let updated = sqlx::query(
            "UPDATE activity_prompt_summaries
             SET state = 'succeeded', summary_text = ?, lease_token = NULL,
                 lease_expires_at_us = NULL, last_error_code = NULL, updated_at_us = ?
             WHERE activity_event_id = ? AND generation = ?
               AND state = 'running' AND lease_token = ?",
        )
        .bind(text.as_str())
        .bind(now_us)
        .bind(claim.activity_event_id)
        .bind(claim.generation)
        .bind(&claim.lease_token)
        .execute(&self.pool)
        .await?;
        Ok(if updated.rows_affected() == 1 {
            PromptSummaryCompletionOutcome::Applied
        } else {
            PromptSummaryCompletionOutcome::Stale
        })
    }

    pub async fn fail_prompt_summary(
        &self,
        claim: &PromptSummaryClaim,
        retry_at_us: Option<i64>,
        code: PromptSummaryErrorCode,
        now_us: i64,
    ) -> Result<PromptSummaryFailureDisposition, StoreError> {
        let should_retry =
            retry_at_us.is_some() && claim.attempt_number < MAX_PROMPT_SUMMARY_ATTEMPTS;
        let state = if should_retry { "retry_wait" } else { "failed" };
        let updated = sqlx::query(
            "UPDATE activity_prompt_summaries
             SET state = ?, lease_token = NULL, lease_expires_at_us = NULL,
                 next_attempt_at_us = ?, last_error_code = ?, updated_at_us = ?
             WHERE activity_event_id = ? AND generation = ?
               AND state = 'running' AND lease_token = ?",
        )
        .bind(state)
        .bind(retry_at_us.filter(|_| should_retry).unwrap_or(0))
        .bind(code.storage_code())
        .bind(now_us)
        .bind(claim.activity_event_id)
        .bind(claim.generation)
        .bind(&claim.lease_token)
        .execute(&self.pool)
        .await?;
        if updated.rows_affected() == 0 {
            return Ok(PromptSummaryFailureDisposition::Stale);
        }
        Ok(if should_retry {
            PromptSummaryFailureDisposition::RetryScheduled
        } else {
            PromptSummaryFailureDisposition::Failed
        })
    }

    pub async fn prompt_summary(
        &self,
        activity_event_id: i64,
    ) -> Result<Option<PromptSummary>, StoreError> {
        let row = sqlx::query(
            "SELECT summaries.state, summaries.projection_kind,
                    COALESCE(summaries.projected_prompt, activities.prompt) AS projected_prompt,
                    summaries.summary_text, summaries.used_previous_result,
                    summaries.context_activity_event_id, summaries.context_result_generation,
                    summaries.source_digest, summaries.generation, summaries.attempt_count,
                    summaries.summary_model, summaries.updated_at_us
             FROM activity_prompt_summaries AS summaries
             JOIN activity_events AS activities ON activities.id = summaries.activity_event_id
             WHERE summaries.activity_event_id = ?",
        )
        .bind(activity_event_id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(prompt_summary_from_storage).transpose()
    }
}

pub(crate) async fn initialize_prompt_summary_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    projection: &PromptProjection,
    now_us: i64,
) -> Result<(), StoreError> {
    if prompt_summary_exists(transaction, activity_event_id).await? {
        return Ok(());
    }
    let activity = activity_input(transaction, activity_event_id).await?;
    initialize_prompt_summary_for_activity(transaction, &activity, projection, now_us).await
}

pub(crate) async fn initialize_new_prompt_summary_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    provider: &str,
    prompt: &str,
    activity_kind: ActivityKind,
    projection: &PromptProjection,
    now_us: i64,
) -> Result<(), StoreError> {
    let activity = ActivityInput {
        id: activity_event_id,
        provider: provider.to_owned(),
        prompt: prompt.to_owned(),
        activity_kind: activity_kind.as_str().to_owned(),
    };
    initialize_prompt_summary_for_activity(transaction, &activity, projection, now_us).await
}

async fn initialize_prompt_summary_for_activity(
    transaction: &mut Transaction<'_, Sqlite>,
    activity: &ActivityInput,
    projection: &PromptProjection,
    now_us: i64,
) -> Result<(), StoreError> {
    let policy = prompt_summary_policy_in_transaction(transaction, &activity.provider).await?;
    let desired = derive_desired_summary(transaction, activity, projection, policy).await?;
    insert_prompt_summary(
        transaction,
        activity.id,
        projection,
        &activity.prompt,
        policy,
        &desired,
        now_us,
    )
    .await
}

pub(crate) async fn reconcile_successor_after_activity(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    now_us: i64,
) -> Result<(), StoreError> {
    let Some(successor_id) = next_user_activity(transaction, activity_event_id).await? else {
        return Ok(());
    };
    reconcile_prompt_summary_context_in_transaction(transaction, successor_id, now_us).await
}

pub(crate) async fn reconcile_context_dependents(
    transaction: &mut Transaction<'_, Sqlite>,
    context_activity_event_id: i64,
    now_us: i64,
) -> Result<(), StoreError> {
    let dependent_ids = sqlx::query_scalar::<_, i64>(
        "SELECT activity_event_id FROM activity_prompt_summaries
         WHERE context_activity_event_id = ?",
    )
    .bind(context_activity_event_id)
    .fetch_all(&mut **transaction)
    .await?;
    for activity_event_id in dependent_ids {
        reconcile_prompt_summary_context_in_transaction(transaction, activity_event_id, now_us)
            .await?;
    }
    Ok(())
}

pub(crate) async fn reconcile_prompt_summary_context_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    now_us: i64,
) -> Result<(), StoreError> {
    let Some(existing) = existing_summary(transaction, activity_event_id).await? else {
        // Historic activities are intentionally never backfilled.
        return Ok(());
    };
    let activity = activity_input(transaction, activity_event_id).await?;
    let projection = PromptProjection::restored(
        existing.projected_prompt.clone(),
        existing.projection_kind,
        existing.projection_version,
    )
    .ok_or_else(|| StoreError::Invariant("stored prompt projection is invalid".into()))?;
    // The policy belongs to the captured activity. Toggling the setting affects
    // future captures only; a late result must not mutate historic work.
    let desired = derive_desired_summary(
        transaction,
        &activity,
        &projection,
        existing.policy_at_capture,
    )
    .await?;
    if existing_matches_desired(&existing, &desired) {
        return Ok(());
    }
    let cached = cached_summary_text(transaction, &desired.source_digest).await?;
    let (state, text) = if desired.state == PromptSummaryState::Pending {
        cached.map_or((desired.state, desired.text.clone()), |text| {
            (PromptSummaryState::Succeeded, Some(text.into_inner()))
        })
    } else {
        (desired.state, desired.text.clone())
    };
    sqlx::query(
        "UPDATE activity_prompt_summaries
         SET state = ?, summary_text = ?, used_previous_result = ?,
             context_activity_event_id = ?, context_result_generation = ?,
             source_digest = ?, generation = generation + 1, attempt_count = 0,
             lease_token = NULL, lease_expires_at_us = NULL, next_attempt_at_us = 0,
             last_error_code = NULL, updated_at_us = ?
         WHERE activity_event_id = ?",
    )
    .bind(state.as_str())
    .bind(text)
    .bind(desired.used_previous_result)
    .bind(desired.context_activity_event_id)
    .bind(desired.context_result_generation)
    .bind(&desired.source_digest)
    .bind(now_us)
    .bind(activity_event_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn prompt_summary_exists(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<bool, StoreError> {
    Ok(sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM activity_prompt_summaries WHERE activity_event_id = ?",
    )
    .bind(activity_event_id)
    .fetch_one(&mut **transaction)
    .await?
        != 0)
}

async fn activity_input(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<ActivityInput, StoreError> {
    let row = sqlx::query(
        "SELECT id, provider, prompt, activity_kind
         FROM activity_events WHERE id = ? AND deleted_at_us IS NULL",
    )
    .bind(activity_event_id)
    .fetch_optional(&mut **transaction)
    .await?
    .ok_or(StoreError::ActivityNotFound(activity_event_id))?;
    Ok(ActivityInput {
        id: row.try_get("id")?,
        provider: row.try_get("provider")?,
        prompt: row.try_get("prompt")?,
        activity_kind: row.try_get("activity_kind")?,
    })
}

async fn derive_desired_summary(
    transaction: &mut Transaction<'_, Sqlite>,
    activity: &ActivityInput,
    projection: &PromptProjection,
    policy: PromptSummaryPolicy,
) -> Result<DesiredSummary, StoreError> {
    let gate = summary_gate(&activity.prompt, projection);
    if activity.activity_kind != ActivityKind::User.as_str()
        || policy == PromptSummaryPolicy::Off
        || !gate.needs_summary
    {
        return Ok(passthrough_desired(projection));
    }

    if gate.needs_previous_result {
        let Some(context) = previous_user_context(transaction, activity.id).await? else {
            return Ok(passthrough_desired(projection));
        };
        return match context.result {
            PreviousResult::Ready { generation, lines } => Ok(DesiredSummary {
                state: PromptSummaryState::Pending,
                text: None,
                used_previous_result: true,
                context_activity_event_id: Some(context.activity_event_id),
                context_result_generation: Some(generation),
                source_digest: source_digest(
                    projection,
                    Some((context.activity_event_id, generation, &lines)),
                ),
            }),
            PreviousResult::Pending => Ok(DesiredSummary {
                state: PromptSummaryState::WaitingContext,
                text: None,
                used_previous_result: true,
                context_activity_event_id: Some(context.activity_event_id),
                context_result_generation: None,
                source_digest: source_digest(projection, None),
            }),
            // A late predecessor can arrive after this activity. Keep a
            // dependency without sending anything to Spark yet so its ready
            // result can deterministically refresh this one later.
            PreviousResult::Missing => Ok(passthrough_desired_with_context(
                projection,
                context.activity_event_id,
            )),
            PreviousResult::TerminalUnavailable => Ok(passthrough_desired(projection)),
        };
    }

    Ok(DesiredSummary {
        state: PromptSummaryState::Pending,
        text: None,
        used_previous_result: false,
        context_activity_event_id: None,
        context_result_generation: None,
        source_digest: source_digest(projection, None),
    })
}

fn passthrough_desired(projection: &PromptProjection) -> DesiredSummary {
    DesiredSummary {
        state: PromptSummaryState::Passthrough,
        text: None,
        used_previous_result: false,
        context_activity_event_id: None,
        context_result_generation: None,
        source_digest: source_digest(projection, None),
    }
}

fn passthrough_desired_with_context(
    projection: &PromptProjection,
    context_activity_event_id: i64,
) -> DesiredSummary {
    DesiredSummary {
        state: PromptSummaryState::Passthrough,
        text: None,
        used_previous_result: false,
        context_activity_event_id: Some(context_activity_event_id),
        context_result_generation: None,
        source_digest: source_digest(projection, None),
    }
}

async fn prompt_summary_policy_in_transaction(
    transaction: &mut Transaction<'_, Sqlite>,
    provider: &str,
) -> Result<PromptSummaryPolicy, StoreError> {
    let value: Option<String> = sqlx::query_scalar(
        "SELECT prompt_summary_mode FROM provider_summary_settings WHERE provider = ?",
    )
    .bind(provider)
    .fetch_optional(&mut **transaction)
    .await?;
    value
        .as_deref()
        .map(PromptSummaryPolicy::from_storage)
        .transpose()
        .map(|value| value.unwrap_or_default())
}

async fn previous_user_context(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<Option<PreviousContext>, StoreError> {
    let row = sqlx::query(
        "WITH current AS (
             SELECT provider, provider_session_id, id,
                    CASE WHEN COALESCE(captured_at_us, first_recorded_at_us) IS NULL
                         THEN 1 ELSE 0 END AS time_class,
                    COALESCE(captured_at_us, first_recorded_at_us) AS ordered_at_us
             FROM activity_events WHERE id = ? AND deleted_at_us IS NULL
         )
         SELECT prior.id, results.activity_event_id AS result_activity_event_id,
                COALESCE(results.state, 'unavailable') AS result_state,
                results.generation AS result_generation,
                results.summary_line_1, results.summary_line_2, results.summary_line_3
         FROM activity_events AS prior
         JOIN current
           ON prior.provider = current.provider
          AND prior.provider_session_id = current.provider_session_id
         LEFT JOIN activity_result_summaries AS results
           ON results.activity_event_id = prior.id
          WHERE prior.activity_kind = 'user'
            AND prior.deleted_at_us IS NULL
           AND prior.id != current.id
           AND (
               (CASE WHEN COALESCE(prior.captured_at_us, prior.first_recorded_at_us) IS NULL
                     THEN 1 ELSE 0 END) < current.time_class
               OR (
                   (CASE WHEN COALESCE(prior.captured_at_us, prior.first_recorded_at_us) IS NULL
                         THEN 1 ELSE 0 END) = current.time_class
                   AND (
                       (
                           prior.captured_at_us IS NULL
                           AND prior.first_recorded_at_us IS NULL
                           AND current.ordered_at_us IS NULL
                           AND prior.id < current.id
                       )
                       OR COALESCE(prior.captured_at_us, prior.first_recorded_at_us) < current.ordered_at_us
                       OR (
                           COALESCE(prior.captured_at_us, prior.first_recorded_at_us) = current.ordered_at_us
                           AND prior.id < current.id
                       )
                   )
               )
           )
         ORDER BY
           CASE WHEN COALESCE(prior.captured_at_us, prior.first_recorded_at_us) IS NULL
                THEN 1 ELSE 0 END DESC,
           COALESCE(prior.captured_at_us, prior.first_recorded_at_us) DESC,
           prior.id DESC
         LIMIT 1",
    )
    .bind(activity_event_id)
    .fetch_optional(&mut **transaction)
    .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let result_activity_event_id: Option<i64> = row.try_get("result_activity_event_id")?;
    let state: String = row.try_get("result_state")?;
    let result = match (result_activity_event_id, state.as_str()) {
        (None, "unavailable") => PreviousResult::Missing,
        (Some(_), "succeeded") => {
            let line_1: Option<String> = row.try_get("summary_line_1")?;
            let line_2: Option<String> = row.try_get("summary_line_2")?;
            let line_3: Option<String> = row.try_get("summary_line_3")?;
            let (Some(line_1), Some(line_2), Some(line_3)) = (line_1, line_2, line_3) else {
                return Err(StoreError::Invariant(
                    "ready previous result is missing one or more lines".into(),
                ));
            };
            PreviousResult::Ready {
                generation: row.try_get("result_generation")?,
                lines: [line_1, line_2, line_3],
            }
        }
        (Some(_), "pending" | "running" | "retry_wait") => PreviousResult::Pending,
        (Some(_), "failed" | "skipped") => PreviousResult::TerminalUnavailable,
        _ => {
            return Err(StoreError::Invariant(format!(
                "invalid previous result state: {state}"
            )));
        }
    };
    Ok(Some(PreviousContext {
        activity_event_id: row.try_get("id")?,
        result,
    }))
}

async fn next_user_activity(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<Option<i64>, StoreError> {
    sqlx::query_scalar(
        "WITH current AS (
             SELECT provider, provider_session_id, id,
                    CASE WHEN COALESCE(captured_at_us, first_recorded_at_us) IS NULL
                         THEN 1 ELSE 0 END AS time_class,
                    COALESCE(captured_at_us, first_recorded_at_us) AS ordered_at_us
             FROM activity_events WHERE id = ? AND deleted_at_us IS NULL
         )
         SELECT successor.id
         FROM activity_events AS successor
         JOIN current
           ON successor.provider = current.provider
          AND successor.provider_session_id = current.provider_session_id
          WHERE successor.activity_kind = 'user'
            AND successor.deleted_at_us IS NULL
           AND (
               (CASE WHEN COALESCE(successor.captured_at_us, successor.first_recorded_at_us) IS NULL
                     THEN 1 ELSE 0 END) > current.time_class
               OR (
                   (CASE WHEN COALESCE(successor.captured_at_us, successor.first_recorded_at_us) IS NULL
                         THEN 1 ELSE 0 END) = current.time_class
                   AND (
                       (
                           successor.captured_at_us IS NULL
                           AND successor.first_recorded_at_us IS NULL
                           AND current.ordered_at_us IS NULL
                           AND successor.id > current.id
                       )
                       OR COALESCE(successor.captured_at_us, successor.first_recorded_at_us) > current.ordered_at_us
                       OR (
                           COALESCE(successor.captured_at_us, successor.first_recorded_at_us) = current.ordered_at_us
                           AND successor.id > current.id
                       )
                   )
               )
           )
         ORDER BY
           CASE WHEN COALESCE(successor.captured_at_us, successor.first_recorded_at_us) IS NULL
                THEN 1 ELSE 0 END,
           COALESCE(successor.captured_at_us, successor.first_recorded_at_us),
           successor.id
         LIMIT 1",
    )
    .bind(activity_event_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(Into::into)
}

async fn insert_prompt_summary(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
    projection: &PromptProjection,
    raw_prompt: &str,
    policy: PromptSummaryPolicy,
    desired: &DesiredSummary,
    now_us: i64,
) -> Result<(), StoreError> {
    let cached = if desired.state == PromptSummaryState::Pending {
        cached_summary_text(transaction, &desired.source_digest).await?
    } else {
        None
    };
    let (state, text) = cached.map_or((desired.state, desired.text.clone()), |text| {
        (PromptSummaryState::Succeeded, Some(text.into_inner()))
    });
    sqlx::query(
        "INSERT OR IGNORE INTO activity_prompt_summaries (
             activity_event_id, state, projection_kind, projected_prompt,
             projection_version, policy_at_capture, summary_text, used_previous_result,
             context_activity_event_id, context_result_generation, source_digest,
             generation, summary_model, attempt_count, lease_token,
             lease_expires_at_us, next_attempt_at_us, last_error_code,
             created_at_us, updated_at_us
         ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 1, ?, 0, NULL, NULL, 0, NULL, ?, ?)",
    )
    .bind(activity_event_id)
    .bind(state.as_str())
    .bind(projection_kind_storage(projection.kind()))
    .bind(
        (projection.kind() != PromptProjectionKind::Raw || projection.text() != raw_prompt)
            .then_some(projection.text()),
    )
    .bind(projection.version())
    .bind(policy.as_str())
    .bind(text)
    .bind(desired.used_previous_result)
    .bind(desired.context_activity_event_id)
    .bind(desired.context_result_generation)
    .bind(&desired.source_digest)
    .bind(PROMPT_SUMMARY_MODEL)
    .bind(now_us)
    .bind(now_us)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

struct ExistingSummary {
    state: PromptSummaryState,
    projection_kind: PromptProjectionKind,
    projected_prompt: String,
    projection_version: i64,
    policy_at_capture: PromptSummaryPolicy,
    text: Option<String>,
    used_previous_result: bool,
    context_activity_event_id: Option<i64>,
    context_result_generation: Option<i64>,
    source_digest: String,
}

async fn existing_summary(
    transaction: &mut Transaction<'_, Sqlite>,
    activity_event_id: i64,
) -> Result<Option<ExistingSummary>, StoreError> {
    let row = sqlx::query(
        "SELECT summaries.state, summaries.projection_kind,
                COALESCE(summaries.projected_prompt, activities.prompt) AS projected_prompt,
                summaries.projection_version, summaries.policy_at_capture,
                summaries.summary_text, summaries.used_previous_result,
                summaries.context_activity_event_id, summaries.context_result_generation,
                summaries.source_digest
         FROM activity_prompt_summaries AS summaries
         JOIN activity_events AS activities ON activities.id = summaries.activity_event_id
         WHERE summaries.activity_event_id = ?",
    )
    .bind(activity_event_id)
    .fetch_optional(&mut **transaction)
    .await?;
    row.map(|row| {
        Ok(ExistingSummary {
            state: PromptSummaryState::from_storage(row.try_get("state")?)?,
            projection_kind: projection_kind_from_storage(
                &row.try_get::<String, _>("projection_kind")?,
            )?,
            projected_prompt: row.try_get("projected_prompt")?,
            projection_version: row.try_get("projection_version")?,
            policy_at_capture: PromptSummaryPolicy::from_storage(
                &row.try_get::<String, _>("policy_at_capture")?,
            )?,
            text: row.try_get("summary_text")?,
            used_previous_result: marker_to_bool(row.try_get("used_previous_result")?)?,
            context_activity_event_id: row.try_get("context_activity_event_id")?,
            context_result_generation: row.try_get("context_result_generation")?,
            source_digest: row.try_get("source_digest")?,
        })
    })
    .transpose()
}

fn existing_matches_desired(existing: &ExistingSummary, desired: &DesiredSummary) -> bool {
    let same_input = existing.source_digest == desired.source_digest
        && existing.used_previous_result == desired.used_previous_result
        && existing.context_activity_event_id == desired.context_activity_event_id
        && existing.context_result_generation == desired.context_result_generation;
    if !same_input {
        return false;
    }
    match existing.state {
        PromptSummaryState::Pending
        | PromptSummaryState::Running
        | PromptSummaryState::RetryWait
            if desired.state == PromptSummaryState::Pending =>
        {
            true
        }
        PromptSummaryState::Succeeded if desired.state == PromptSummaryState::Pending => {
            existing.text.is_some()
        }
        state => state == desired.state && existing.text == desired.text,
    }
}

async fn cached_summary_text(
    transaction: &mut Transaction<'_, Sqlite>,
    source_digest: &str,
) -> Result<Option<PromptSummaryText>, StoreError> {
    let text: Option<String> = sqlx::query_scalar(
        "SELECT summary_text FROM activity_prompt_summaries
         WHERE source_digest = ? AND state = 'succeeded' AND summary_text IS NOT NULL
         ORDER BY updated_at_us DESC, activity_event_id DESC
         LIMIT 1",
    )
    .bind(source_digest)
    .fetch_optional(&mut **transaction)
    .await?;
    // A stale pre-release row must never prevent a new capture from being
    // summarized. Only current-contract output is eligible for cache reuse.
    Ok(text.and_then(|text| PromptSummaryText::try_new(text).ok()))
}

fn prompt_summary_from_storage(row: sqlx::sqlite::SqliteRow) -> Result<PromptSummary, StoreError> {
    let text: Option<String> = row.try_get("summary_text")?;
    Ok(PromptSummary {
        state: PromptSummaryState::from_storage(row.try_get("state")?)?,
        projection_kind: projection_kind_from_storage(
            &row.try_get::<String, _>("projection_kind")?,
        )?,
        projected_prompt: row.try_get("projected_prompt")?,
        text: text.and_then(|text| PromptSummaryText::try_new(text).ok()),
        used_previous_result: marker_to_bool(row.try_get("used_previous_result")?)?,
        context_activity_event_id: row.try_get("context_activity_event_id")?,
        context_result_generation: row.try_get("context_result_generation")?,
        source_digest: row.try_get("source_digest")?,
        generation: row.try_get("generation")?,
        attempt_count: row.try_get("attempt_count")?,
        summary_model: row.try_get("summary_model")?,
        updated_at_us: row.try_get("updated_at_us")?,
    })
}

pub(crate) fn activity_prompt_summary_from_parts(
    state: &str,
    projected_prompt: Option<String>,
    summary_text: Option<String>,
    used_previous_result: Option<i64>,
) -> Result<ActivityPromptSummary, StoreError> {
    let Some(projected_prompt) = projected_prompt else {
        return Ok(ActivityPromptSummary::unavailable());
    };
    let state = PromptSummaryState::from_storage(state)?;
    let used_previous_result = used_previous_result
        .map(marker_to_bool)
        .transpose()?
        .unwrap_or(false);
    let (status, mode, text) = match state {
        PromptSummaryState::Passthrough => (
            PromptSummaryStatus::Ready,
            PromptSummaryMode::Passthrough,
            Some(projected_prompt),
        ),
        PromptSummaryState::WaitingContext
        | PromptSummaryState::Pending
        | PromptSummaryState::Running
        | PromptSummaryState::RetryWait => (
            PromptSummaryStatus::Pending,
            PromptSummaryMode::Passthrough,
            Some(projected_prompt),
        ),
        PromptSummaryState::Succeeded => {
            let Some(text) = summary_text.and_then(|text| PromptSummaryText::try_new(text).ok())
            else {
                // Do not let an invalid derived row from an earlier build turn
                // an immutable activity into an API error. The raw evidence
                // remains unchanged; the UI truthfully shows its projection.
                return Ok(ActivityPromptSummary {
                    status: PromptSummaryStatus::Failed,
                    mode: PromptSummaryMode::Fallback,
                    text: Some(projected_prompt),
                });
            };
            (
                PromptSummaryStatus::Ready,
                if used_previous_result {
                    PromptSummaryMode::Contextual
                } else {
                    PromptSummaryMode::Standalone
                },
                Some(text.into_inner()),
            )
        }
        PromptSummaryState::Failed => (
            PromptSummaryStatus::Failed,
            PromptSummaryMode::Fallback,
            Some(projected_prompt),
        ),
    };
    Ok(ActivityPromptSummary { status, mode, text })
}

fn marker_to_bool(value: i64) -> Result<bool, StoreError> {
    match value {
        0 => Ok(false),
        1 => Ok(true),
        _ => Err(StoreError::Invariant(format!(
            "invalid prompt summary boolean marker: {value}"
        ))),
    }
}

fn projection_kind_storage(kind: PromptProjectionKind) -> &'static str {
    match kind {
        PromptProjectionKind::Raw => "raw",
        PromptProjectionKind::CodexWrapperRemoved => "codex_wrapper_removed",
    }
}

fn projection_kind_from_storage(value: &str) -> Result<PromptProjectionKind, StoreError> {
    match value {
        "raw" => Ok(PromptProjectionKind::Raw),
        "codex_wrapper_removed" => Ok(PromptProjectionKind::CodexWrapperRemoved),
        _ => Err(StoreError::Invariant(format!(
            "invalid prompt projection kind: {value}"
        ))),
    }
}

fn source_digest(
    projection: &PromptProjection,
    context: Option<(i64, i64, &[String; 3])>,
) -> String {
    let mut hasher = Sha256::new();
    for value in [
        PROMPT_SUMMARY_VERSION_PREFIX,
        PROMPT_SUMMARY_MODEL,
        &projection.version().to_string(),
        projection.text(),
    ] {
        hasher.update(value.as_bytes());
        hasher.update([0]);
    }
    if let Some((activity_event_id, generation, lines)) = context {
        for value in [activity_event_id.to_string(), generation.to_string()] {
            hasher.update(value.as_bytes());
            hasher.update([0]);
        }
        for line in lines {
            hasher.update(line.as_bytes());
            hasher.update([0]);
        }
    } else {
        hasher.update([0]);
    }
    format!("{:x}", hasher.finalize())
}

fn summary_gate(raw_prompt: &str, projection: &PromptProjection) -> SummaryGate {
    let projected = projection.text().trim();
    let projected_characters = projected.chars().count();
    if projected.is_empty() || projected_characters > MAX_PROMPT_SUMMARY_INPUT_CHARS {
        return SummaryGate {
            needs_summary: false,
            needs_previous_result: false,
        };
    }
    let contextual = is_contextual_prompt(projected);
    let raw_characters = raw_prompt.chars().count();
    let difference = raw_characters.saturating_sub(projected_characters);
    let wrapper_heavy = projection.kind() == PromptProjectionKind::CodexWrapperRemoved
        && raw_characters > 0
        && difference.saturating_mul(100) >= raw_characters.saturating_mul(35);
    SummaryGate {
        needs_summary: contextual
            || projected_characters > 220
            || difference >= 160
            || wrapper_heavy,
        needs_previous_result: contextual,
    }
}

fn is_contextual_prompt(prompt: &str) -> bool {
    let normalized = prompt.trim().to_lowercase();
    let short_continuation = normalized.chars().count() <= 80
        && ["진행해", "계속", "네", "좋아요", "그렇게 해주세요"]
            .iter()
            .any(|value| normalized == *value || normalized.starts_with(&format!("{value} ")));
    short_continuation
        || ["이 방식", "해당 내용", "위 작업", "앞의 답변", "그거"]
            .iter()
            .any(|value| normalized.contains(value))
}

#[cfg(test)]
mod tests {
    use akra_core::{ingress::IngressEvent, prompt_projection::PromptProjection};
    use akra_git::ProjectIdentity;

    use super::{
        MAX_PROMPT_SUMMARY_CHARS, PromptSummaryMode, PromptSummaryPolicy, PromptSummaryStatus,
        PromptSummaryText, PromptSummaryValidationError, activity_prompt_summary_from_parts,
        summary_gate,
    };
    use crate::{ActivityOrder, ActivityScope, ActivityStore, RecordActivity};

    #[test]
    fn validator_uses_unicode_scalars_and_rejects_display_breakers() {
        assert!(PromptSummaryText::try_new("가".repeat(MAX_PROMPT_SUMMARY_CHARS)).is_ok());
        assert_eq!(
            PromptSummaryText::try_new("가".repeat(MAX_PROMPT_SUMMARY_CHARS + 1)),
            Err(PromptSummaryValidationError::SummaryTooLong(
                MAX_PROMPT_SUMMARY_CHARS + 1
            ))
        );
        assert_eq!(
            PromptSummaryText::try_new(" summary"),
            Err(PromptSummaryValidationError::SurroundingWhitespace)
        );
        assert_eq!(
            PromptSummaryText::try_new("- bullet"),
            Err(PromptSummaryValidationError::MarkdownPrefix)
        );
        assert_eq!(
            PromptSummaryText::try_new("Add a health endpoint."),
            Err(PromptSummaryValidationError::NonKorean)
        );
        assert_eq!(
            PromptSummaryText::try_new("첫 문장입니다. 두 번째 문장입니다."),
            Err(PromptSummaryValidationError::MultipleSentences)
        );
        assert!(PromptSummaryText::try_new("v1.2 검증을 진행합니다.").is_ok());
    }

    #[test]
    fn smart_gate_keeps_independent_short_requests_as_passthrough() {
        let projection = PromptProjection::raw("README에 설치 절차를 추가해줘");
        let decision = summary_gate(projection.text(), &projection);
        assert!(!decision.needs_summary);

        let continuation = PromptProjection::raw("네 진행하세요");
        let decision = summary_gate(continuation.text(), &continuation);
        assert!(decision.needs_summary);
        assert!(decision.needs_previous_result);
    }

    #[test]
    fn an_invalid_persisted_success_falls_back_without_breaking_activity_reads() {
        let summary = activity_prompt_summary_from_parts(
            "succeeded",
            Some("검증 작업을 진행".to_owned()),
            Some("Add a health endpoint.".to_owned()),
            Some(0),
        )
        .expect("fallback summary");

        assert_eq!(summary.status, PromptSummaryStatus::Failed);
        assert_eq!(summary.mode, PromptSummaryMode::Fallback);
        assert_eq!(summary.text.as_deref(), Some("검증 작업을 진행"));
    }

    #[tokio::test]
    async fn raw_projection_is_stored_once_and_restored_by_every_prompt_reader() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        store
            .set_prompt_summary_policy("codex", PromptSummaryPolicy::Smart)
            .await
            .expect("smart policy");
        let cwd = std::env::current_dir().expect("cwd");
        let prompt = "가".repeat(300);
        let event = IngressEvent::try_new(
            "codex",
            "projection-storage",
            "raw",
            cwd.to_string_lossy(),
            &prompt,
            None,
        )
        .expect("event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let activity_id = store
            .record(
                RecordActivity::captured(event, origin, 1)
                    .with_prompt_projection(PromptProjection::raw(&prompt)),
            )
            .await
            .expect("record");

        let stored: Option<String> = sqlx::query_scalar(
            "SELECT projected_prompt FROM activity_prompt_summaries
             WHERE activity_event_id = ?",
        )
        .bind(activity_id)
        .fetch_one(&store.pool)
        .await
        .expect("stored projection");
        assert_eq!(stored, None);
        assert_eq!(
            store
                .prompt_summary(activity_id)
                .await
                .expect("prompt summary")
                .expect("summary row")
                .projected_prompt,
            prompt
        );
        assert_eq!(
            store
                .claim_prompt_summary(1, 10)
                .await
                .expect("claim")
                .expect("pending claim")
                .projected_prompt(),
            prompt
        );
        for summaries in [
            store
                .activity_summaries_ordered_page(
                    ActivityScope::All,
                    None,
                    10,
                    ActivityOrder::Oldest,
                )
                .await
                .expect("ordered summaries"),
            store
                .activity_summaries_indexed_page(
                    ActivityScope::All,
                    None,
                    10,
                    ActivityOrder::Oldest,
                )
                .await
                .expect("indexed summaries"),
        ] {
            assert_eq!(
                summaries[0].prompt_summary.text.as_deref(),
                Some(prompt.as_str())
            );
        }
        assert_eq!(
            store
                .activity_detail(activity_id)
                .await
                .expect("activity detail")
                .prompt_summary
                .text
                .as_deref(),
            Some(prompt.as_str())
        );
    }

    #[tokio::test]
    async fn a_raw_projection_that_differs_from_the_activity_remains_materialized() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("cwd");
        let event = IngressEvent::try_new(
            "codex",
            "projection-storage",
            "alternate",
            cwd.to_string_lossy(),
            "captured prompt",
            None,
        )
        .expect("event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let activity_id = store
            .record(RecordActivity::captured(event, origin, 1))
            .await
            .expect("record");
        sqlx::query("DELETE FROM activity_prompt_summaries WHERE activity_event_id = ?")
            .bind(activity_id)
            .execute(&store.pool)
            .await
            .expect("simulate historic activity");

        store
            .initialize_prompt_summary(
                activity_id,
                &PromptProjection::raw("alternate raw prompt"),
                2,
            )
            .await
            .expect("initialize projection");

        let stored: Option<String> = sqlx::query_scalar(
            "SELECT projected_prompt FROM activity_prompt_summaries
             WHERE activity_event_id = ?",
        )
        .bind(activity_id)
        .fetch_one(&store.pool)
        .await
        .expect("stored projection");
        assert_eq!(stored.as_deref(), Some("alternate raw prompt"));
    }

    #[tokio::test]
    async fn a_derived_projection_remains_materialized() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("cwd");
        let event = IngressEvent::try_new(
            "codex",
            "projection-storage",
            "derived",
            cwd.to_string_lossy(),
            "wrapped captured prompt",
            None,
        )
        .expect("event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let projection = PromptProjection::codex_wrapper_removed("derived prompt", 8)
            .expect("derived projection");
        let activity_id = store
            .record(RecordActivity::captured(event, origin, 1).with_prompt_projection(projection))
            .await
            .expect("record");

        let stored: Option<String> = sqlx::query_scalar(
            "SELECT projected_prompt FROM activity_prompt_summaries
             WHERE activity_event_id = ?",
        )
        .bind(activity_id)
        .fetch_one(&store.pool)
        .await
        .expect("stored projection");
        assert_eq!(stored.as_deref(), Some("derived prompt"));
        assert_eq!(
            store
                .prompt_summary(activity_id)
                .await
                .expect("prompt summary")
                .expect("summary row")
                .projected_prompt,
            "derived prompt"
        );
    }
}
