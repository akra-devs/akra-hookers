use sqlx::Row;

use crate::{ActivityStore, StoreError};

const MAX_STORED_CODEX_ERROR_CHARS: usize = 2_048;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExecOperation {
    ResultSummary,
    PromptSummary,
    WorkCuration,
}

impl CodexExecOperation {
    const fn as_str(self) -> &'static str {
        match self {
            Self::ResultSummary => "result_summary",
            Self::PromptSummary => "prompt_summary",
            Self::WorkCuration => "work_curation",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CodexExecStatus {
    Succeeded,
    Failed,
    TimedOut,
    QuotaLimited,
}

impl CodexExecStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::QuotaLimited => "quota_limited",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct CodexTokenUsage {
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_output_tokens: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CodexExecCallRecord {
    pub operation: CodexExecOperation,
    pub activity_event_id: Option<i64>,
    pub model: String,
    pub capture_target: Option<String>,
    pub attempt_number: i64,
    pub source_chars: i64,
    pub submitted_source_chars: i64,
    pub prompt_chars: i64,
    pub started_at_us: i64,
    pub completed_at_us: i64,
    pub status: CodexExecStatus,
    pub exit_code: Option<i32>,
    pub thread_id: Option<String>,
    pub usage: Option<CodexTokenUsage>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
    pub quota_retry_at_us: Option<i64>,
}

impl ActivityStore {
    pub async fn record_codex_exec_call(
        &self,
        record: &CodexExecCallRecord,
    ) -> Result<(), StoreError> {
        validate_record(record)?;
        let error_code = record.error_code.as_deref().map(sanitize_error);
        let error_message = record.error_message.as_deref().map(sanitize_error);
        let mut transaction = self.pool.begin().await?;
        let usage = record.usage;
        sqlx::query(
            "INSERT INTO codex_exec_calls (
                 operation, activity_event_id, model, capture_target, attempt_number,
                 source_chars, submitted_source_chars, prompt_chars,
                 started_at_us, completed_at_us, status, exit_code, thread_id,
                 input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens,
                 error_code, error_message, quota_retry_at_us
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(record.operation.as_str())
        .bind(record.activity_event_id)
        .bind(&record.model)
        .bind(&record.capture_target)
        .bind(record.attempt_number)
        .bind(record.source_chars)
        .bind(record.submitted_source_chars)
        .bind(record.prompt_chars)
        .bind(record.started_at_us)
        .bind(record.completed_at_us)
        .bind(record.status.as_str())
        .bind(record.exit_code)
        .bind(&record.thread_id)
        .bind(usage.map(|usage| usage.input_tokens))
        .bind(usage.map(|usage| usage.cached_input_tokens))
        .bind(usage.map(|usage| usage.output_tokens))
        .bind(usage.map(|usage| usage.reasoning_output_tokens))
        .bind(error_code)
        .bind(error_message.as_deref())
        .bind(record.quota_retry_at_us)
        .execute(&mut *transaction)
        .await?;

        if record.status == CodexExecStatus::QuotaLimited {
            let retry_at_us = record
                .quota_retry_at_us
                .expect("validated quota retry timestamp");
            sqlx::query(
                "INSERT INTO codex_quota_circuits (
                     model, opened_at_us, retry_at_us, last_error, updated_at_us
                 ) VALUES (?, ?, ?, ?, ?)
                 ON CONFLICT(model) DO UPDATE SET
                     opened_at_us = excluded.opened_at_us,
                     retry_at_us = excluded.retry_at_us,
                     last_error = excluded.last_error,
                     updated_at_us = excluded.updated_at_us",
            )
            .bind(&record.model)
            .bind(record.completed_at_us)
            .bind(retry_at_us)
            .bind(error_message.unwrap_or_else(|| "Codex usage limit exceeded".to_owned()))
            .bind(record.completed_at_us)
            .execute(&mut *transaction)
            .await?;
        } else {
            // A successful half-open probe, or a non-quota failure proving the
            // service is reachable, closes an expired circuit.
            sqlx::query(
                "DELETE FROM codex_quota_circuits
                 WHERE model = ? AND retry_at_us <= ?",
            )
            .bind(&record.model)
            .bind(record.started_at_us)
            .execute(&mut *transaction)
            .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn active_codex_quota_retry_at(
        &self,
        model: &str,
        now_us: i64,
    ) -> Result<Option<i64>, StoreError> {
        Ok(sqlx::query(
            "SELECT retry_at_us FROM codex_quota_circuits
             WHERE model = ? AND retry_at_us > ?",
        )
        .bind(model)
        .bind(now_us)
        .fetch_optional(&self.pool)
        .await?
        .map(|row| row.get("retry_at_us")))
    }
}

fn validate_record(record: &CodexExecCallRecord) -> Result<(), StoreError> {
    if record.model.trim().is_empty() {
        return Err(invalid_record("model is blank"));
    }
    if record.attempt_number < 1
        || record.source_chars < 0
        || record.submitted_source_chars < 0
        || record.prompt_chars < 0
        || record.completed_at_us < record.started_at_us
    {
        return Err(invalid_record(
            "numeric fields are outside their valid range",
        ));
    }
    if record.submitted_source_chars > record.source_chars {
        return Err(invalid_record(
            "submitted source length exceeds the captured source length",
        ));
    }
    if let Some(usage) = record.usage
        && (usage.input_tokens < 0
            || usage.cached_input_tokens < 0
            || usage.output_tokens < 0
            || usage.reasoning_output_tokens < 0
            || usage.cached_input_tokens > usage.input_tokens)
    {
        return Err(invalid_record("token usage is invalid"));
    }
    match (record.status, record.quota_retry_at_us) {
        (CodexExecStatus::QuotaLimited, Some(retry_at_us))
            if retry_at_us > record.completed_at_us => {}
        (CodexExecStatus::QuotaLimited, _) => {
            return Err(invalid_record("quota retry timestamp is invalid"));
        }
        (_, None) => {}
        (_, Some(_)) => {
            return Err(invalid_record(
                "non-quota call contains a quota retry timestamp",
            ));
        }
    }
    Ok(())
}

fn invalid_record(message: &str) -> StoreError {
    StoreError::InvalidCodexExecCall(message.to_owned())
}

fn sanitize_error(value: &str) -> String {
    let compact = value
        .chars()
        .map(|character| {
            if matches!(character, '\r' | '\n' | '\t') || character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if compact.chars().count() <= MAX_STORED_CODEX_ERROR_CHARS {
        return compact;
    }
    compact
        .chars()
        .take(MAX_STORED_CODEX_ERROR_CHARS - 1)
        .chain(std::iter::once('…'))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{CodexExecCallRecord, CodexExecOperation, CodexExecStatus, CodexTokenUsage};
    use crate::ActivityStore;

    fn record(status: CodexExecStatus) -> CodexExecCallRecord {
        CodexExecCallRecord {
            operation: CodexExecOperation::ResultSummary,
            activity_event_id: None,
            model: "gpt-5.3-codex-spark".to_owned(),
            capture_target: Some("native".to_owned()),
            attempt_number: 1,
            source_chars: 700,
            submitted_source_chars: 700,
            prompt_chars: 1_000,
            started_at_us: 10,
            completed_at_us: 20,
            status,
            exit_code: Some(0),
            thread_id: Some("thread-1".to_owned()),
            usage: Some(CodexTokenUsage {
                input_tokens: 12_000,
                cached_input_tokens: 10_000,
                output_tokens: 120,
                reasoning_output_tokens: 20,
            }),
            error_code: None,
            error_message: None,
            quota_retry_at_us: None,
        }
    }

    #[tokio::test]
    async fn records_tokens_and_persists_then_closes_the_quota_circuit() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        let mut quota = record(CodexExecStatus::QuotaLimited);
        quota.exit_code = Some(1);
        quota.usage = None;
        quota.error_code = Some("UsageLimitExceeded".to_owned());
        quota.error_message = Some("usage limit exceeded".to_owned());
        quota.quota_retry_at_us = Some(3_600_000_020);
        store
            .record_codex_exec_call(&quota)
            .await
            .expect("quota call");
        assert_eq!(
            store
                .active_codex_quota_retry_at("gpt-5.3-codex-spark", 21)
                .await
                .expect("circuit"),
            Some(3_600_000_020)
        );

        let mut success = record(CodexExecStatus::Succeeded);
        success.started_at_us = 3_600_000_020;
        success.completed_at_us = 3_600_000_030;
        store
            .record_codex_exec_call(&success)
            .await
            .expect("success call");
        assert_eq!(
            store
                .active_codex_quota_retry_at("gpt-5.3-codex-spark", 3_600_000_021)
                .await
                .expect("closed circuit"),
            None
        );

        let row: (i64, i64, i64, i64) = sqlx::query_as(
            "SELECT input_tokens, cached_input_tokens, output_tokens, reasoning_output_tokens
             FROM codex_exec_calls WHERE status = 'succeeded'",
        )
        .fetch_one(&store.pool)
        .await
        .expect("stored usage");
        assert_eq!(row, (12_000, 10_000, 120, 20));
    }
}
