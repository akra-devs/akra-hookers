use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::Serialize;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, QueryBuilder, Sqlite, SqliteConnection, Transaction};

use crate::{
    ActivityProjectSummary, ActivityPromptSummary, ActivityResultSummary, ActivityStore,
    ActivityTimeRange, CurationApplyResult, CurationLogState, CurationLogSummary, CurationProposal,
    CurationProposalGroup, StoreError, WorkEdgeSummary, WorkItemDetail, WorkItemSummary,
    WorkLogSummary, activities::activity_time, activity_details::result_summary_from_parts,
    canvas::bump_canvas_revision, prompt_summaries::activity_prompt_summary_from_parts,
};

pub const MAX_CURATION_LOGS: usize = 20;
pub const MAX_CURATION_CANDIDATES: usize = 5;
pub const MAX_WORK_TITLE_CHARS: usize = 80;
pub const CURATION_MODEL: &str = "gpt-5.3-codex-spark";
const CURATION_PROMPT_VERSION: u32 = 1;
const MAX_FALLBACK_PROMPT_CHARS: usize = 96;
const MAX_WORK_SIGNATURE_CHARS: usize = 240;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CurationLogFilter {
    Unreviewed,
    Excluded,
    Organized,
    All,
}

impl CurationLogFilter {
    const fn as_storage(self) -> &'static str {
        match self {
            Self::Unreviewed => "unreviewed",
            Self::Excluded => "excluded",
            Self::Organized => "organized",
            Self::All => "all",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CurationModelInput {
    pub project_id: i64,
    pub logs: Vec<CurationModelLog>,
    pub existing_works: Vec<CurationModelWork>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CurationModelLog {
    pub id: i64,
    pub sequence: usize,
    pub session_group: usize,
    pub prompt_summary: String,
    pub result_summary: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct CurationModelWork {
    pub id: i64,
    pub title: String,
    pub signature: String,
    #[serde(skip_serializing)]
    pub updated_at_us: i64,
}

#[derive(Clone, Debug)]
pub struct CurationPreparation {
    fingerprint: String,
    selected_activity_ids: Vec<i64>,
    input: CurationModelInput,
    cached: Option<CurationProposal>,
}

impl CurationPreparation {
    pub fn fingerprint(&self) -> &str {
        &self.fingerprint
    }

    pub fn selected_activity_ids(&self) -> &[i64] {
        &self.selected_activity_ids
    }

    pub fn input(&self) -> &CurationModelInput {
        &self.input
    }

    pub fn cached(&self) -> Option<&CurationProposal> {
        self.cached.as_ref()
    }
}

#[derive(FromRow)]
struct LogRow {
    id: i64,
    project_id: i64,
    project_name: String,
    provider_session_id: String,
    prompt: String,
    captured_at_us: Option<i64>,
    first_recorded_at_us: Option<i64>,
    prompt_summary_state: Option<String>,
    projected_prompt: Option<String>,
    prompt_summary_text: Option<String>,
    prompt_summary_used_previous_result: Option<i64>,
    prompt_summary_source_digest: Option<String>,
    prompt_summary_generation: Option<i64>,
    result_summary_state: String,
    summary_line_1: Option<String>,
    summary_line_2: Option<String>,
    summary_line_3: Option<String>,
    result_summary_source_retained: i64,
    result_summary_source_digest: Option<String>,
    result_summary_generation: Option<i64>,
    curation_state: String,
}

#[derive(FromRow)]
struct WorkRow {
    id: i64,
    project_id: i64,
    project_name: String,
    title: String,
    position_x: f64,
    position_y: f64,
    updated_at_us: i64,
    log_count: i64,
}

#[derive(FromRow)]
struct CandidateRow {
    id: i64,
    title: String,
    updated_at_us: i64,
}

impl ActivityStore {
    pub async fn curation_logs(
        &self,
        project_id: i64,
        filter: CurationLogFilter,
        limit: i64,
    ) -> Result<Vec<CurationLogSummary>, StoreError> {
        self.curation_logs_in_range(project_id, filter, ActivityTimeRange::ALL, limit)
            .await
    }

    pub async fn curation_logs_in_range(
        &self,
        project_id: i64,
        filter: CurationLogFilter,
        time_range: ActivityTimeRange,
        limit: i64,
    ) -> Result<Vec<CurationLogSummary>, StoreError> {
        ensure_positive_limit(limit)?;
        ensure_project_exists_on_pool(&self.pool, project_id).await?;
        let rows = sqlx::query_as::<_, LogRow>(&format!(
            "{LOG_SELECT}
             WHERE effective_project_id = ?
               AND activity_events.activity_kind = 'user'
               AND activity_events.deleted_at_us IS NULL
               AND (? = 'all' OR curation_state = ?)
               AND (
                   ? IS NULL
                   OR COALESCE(
                       activity_events.captured_at_us,
                       activity_events.first_recorded_at_us
                   ) >= ?
               )
             ORDER BY COALESCE(
                 activity_events.captured_at_us,
                 activity_events.first_recorded_at_us
             ) DESC, activity_events.id DESC
             LIMIT ?"
        ))
        .bind(project_id)
        .bind(filter.as_storage())
        .bind(filter.as_storage())
        .bind(time_range.start_at_us())
        .bind(time_range.start_at_us())
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(curation_log_from_row).collect()
    }

    pub async fn set_activity_excluded(
        &self,
        activity_id: i64,
        excluded: bool,
    ) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        ensure_user_activity_available(&mut transaction, activity_id).await?;
        let organized: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_logs WHERE activity_event_id = ?")
                .bind(activity_id)
                .fetch_one(&mut *transaction)
                .await?;
        if organized != 0 {
            return Err(StoreError::InvalidCuration(
                "Remove the log from its work before excluding it.".into(),
            ));
        }
        if excluded {
            sqlx::query(
                "INSERT INTO activity_curation_states(activity_event_id, state, updated_at_us)
                 VALUES (?, 'excluded', ?)
                 ON CONFLICT(activity_event_id) DO UPDATE SET
                     state = 'excluded', updated_at_us = excluded.updated_at_us",
            )
            .bind(activity_id)
            .bind(now_us()?)
            .execute(&mut *transaction)
            .await?;
        } else {
            sqlx::query("DELETE FROM activity_curation_states WHERE activity_event_id = ?")
                .bind(activity_id)
                .execute(&mut *transaction)
                .await?;
        }
        transaction.commit().await?;
        Ok(())
    }

    pub async fn soft_delete_activity(&self, activity_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        let exists: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM activity_events
             WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(activity_id)
        .fetch_one(&mut *transaction)
        .await?;
        if exists == 0 {
            return Err(StoreError::ActivityNotFound(activity_id));
        }
        let deleted_at_us = now_us()?;
        let work_ids = sqlx::query_scalar::<_, i64>(
            "SELECT work_item_id FROM work_item_logs WHERE activity_event_id = ?",
        )
        .bind(activity_id)
        .fetch_all(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM work_item_logs WHERE activity_event_id = ?")
            .bind(activity_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("DELETE FROM activity_curation_states WHERE activity_event_id = ?")
            .bind(activity_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query("UPDATE activity_events SET deleted_at_us = ? WHERE id = ?")
            .bind(deleted_at_us)
            .bind(activity_id)
            .execute(&mut *transaction)
            .await?;
        sqlx::query(
            "UPDATE activity_result_summaries
             SET state = 'skipped', source_text = NULL,
                 summary_line_1 = NULL, summary_line_2 = NULL, summary_line_3 = NULL,
                 lease_token = NULL, lease_expires_at_us = NULL,
                 last_error = 'activity soft-deleted before summarization',
                 updated_at_us = ?, completed_at_us = ?
             WHERE activity_event_id = ?
               AND state IN ('pending', 'running', 'retry_wait')",
        )
        .bind(deleted_at_us)
        .bind(deleted_at_us)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?;

        let canvas_node_ids = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM canvas_nodes WHERE activity_event_id = ? AND deleted_at_us IS NULL",
        )
        .bind(activity_id)
        .fetch_all(&mut *transaction)
        .await?;
        for node_id in canvas_node_ids {
            sqlx::query("DELETE FROM canvas_edges WHERE source_node_id = ? OR target_node_id = ?")
                .bind(node_id)
                .bind(node_id)
                .execute(&mut *transaction)
                .await?;
        }
        sqlx::query(
            "UPDATE canvas_nodes SET deleted_at_us = ?
             WHERE activity_event_id = ? AND deleted_at_us IS NULL",
        )
        .bind(deleted_at_us)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?;

        let work_changed = !work_ids.is_empty();
        for work_id in work_ids {
            let remaining: i64 =
                sqlx::query_scalar("SELECT COUNT(*) FROM work_item_logs WHERE work_item_id = ?")
                    .bind(work_id)
                    .fetch_one(&mut *transaction)
                    .await?;
            if remaining == 0 {
                soft_delete_work_in(&mut transaction, work_id, deleted_at_us).await?;
            } else {
                sqlx::query("UPDATE work_items SET updated_at_us = ? WHERE id = ?")
                    .bind(deleted_at_us)
                    .bind(work_id)
                    .execute(&mut *transaction)
                    .await?;
            }
        }
        if work_changed {
            bump_work_revision(&mut transaction).await?;
        }
        bump_canvas_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn work_items(
        &self,
        project_id: Option<i64>,
    ) -> Result<Vec<WorkItemSummary>, StoreError> {
        if let Some(project_id) = project_id {
            ensure_project_exists_on_pool(&self.pool, project_id).await?;
        }
        let rows = sqlx::query_as::<_, WorkRow>(
            "SELECT work_items.id, work_items.project_id, projects.name AS project_name,
                    work_items.title, work_items.position_x, work_items.position_y,
                    work_items.updated_at_us, COUNT(work_item_logs.activity_event_id) AS log_count
             FROM work_items
             JOIN projects ON projects.id = work_items.project_id
             JOIN work_item_logs ON work_item_logs.work_item_id = work_items.id
             JOIN activity_events ON activity_events.id = work_item_logs.activity_event_id
             WHERE work_items.deleted_at_us IS NULL
               AND activity_events.deleted_at_us IS NULL
               AND (? IS NULL OR work_items.project_id = ?)
             GROUP BY work_items.id, projects.name
             ORDER BY work_items.updated_at_us DESC, work_items.id DESC",
        )
        .bind(project_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        let mut summaries = Vec::with_capacity(rows.len());
        for row in rows {
            summaries.push(self.work_summary_from_row(row, 2).await?);
        }
        Ok(summaries)
    }

    pub async fn work_item(&self, work_id: i64) -> Result<WorkItemDetail, StoreError> {
        let row = self.work_row(work_id).await?;
        let logs = self.work_logs(work_id, i64::MAX).await?;
        let summary = WorkItemSummary {
            id: row.id,
            project: ActivityProjectSummary {
                id: row.project_id,
                name: row.project_name,
            },
            title: row.title,
            log_count: row.log_count,
            position_x: row.position_x,
            position_y: row.position_y,
            updated_at_us: row.updated_at_us,
            preview_logs: logs.iter().take(2).cloned().collect(),
        };
        Ok(WorkItemDetail { summary, logs })
    }

    pub async fn update_work_position(
        &self,
        work_id: i64,
        position_x: f64,
        position_y: f64,
    ) -> Result<(), StoreError> {
        if !position_x.is_finite() || !position_y.is_finite() {
            return Err(StoreError::InvalidCuration(
                "Work position must be finite.".into(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        ensure_work_exists(&mut transaction, work_id).await?;
        sqlx::query(
            "UPDATE work_items SET position_x = ?, position_y = ?, updated_at_us = ?
             WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(position_x)
        .bind(position_y)
        .bind(now_us()?)
        .bind(work_id)
        .execute(&mut *transaction)
        .await?;
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn rename_work(&self, work_id: i64, title: &str) -> Result<(), StoreError> {
        let title = parse_work_title(title)?;
        let mut transaction = self.pool.begin().await?;
        ensure_work_exists(&mut transaction, work_id).await?;
        sqlx::query(
            "UPDATE work_items SET title = ?, updated_at_us = ?
             WHERE id = ? AND deleted_at_us IS NULL",
        )
        .bind(title)
        .bind(now_us()?)
        .bind(work_id)
        .execute(&mut *transaction)
        .await?;
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_work(&self, work_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        ensure_work_exists(&mut transaction, work_id).await?;
        sqlx::query("DELETE FROM work_item_logs WHERE work_item_id = ?")
            .bind(work_id)
            .execute(&mut *transaction)
            .await?;
        soft_delete_work_in(&mut transaction, work_id, now_us()?).await?;
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn remove_work_log(&self, work_id: i64, activity_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        ensure_work_exists(&mut transaction, work_id).await?;
        let removed = sqlx::query(
            "DELETE FROM work_item_logs WHERE work_item_id = ? AND activity_event_id = ?",
        )
        .bind(work_id)
        .bind(activity_id)
        .execute(&mut *transaction)
        .await?
        .rows_affected();
        if removed == 0 {
            return Err(StoreError::InvalidCuration(
                "The activity is not part of this work.".into(),
            ));
        }
        let remaining: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_logs WHERE work_item_id = ?")
                .bind(work_id)
                .fetch_one(&mut *transaction)
                .await?;
        if remaining == 0 {
            soft_delete_work_in(&mut transaction, work_id, now_us()?).await?;
        } else {
            sqlx::query("UPDATE work_items SET updated_at_us = ? WHERE id = ?")
                .bind(now_us()?)
                .bind(work_id)
                .execute(&mut *transaction)
                .await?;
        }
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn work_edges(
        &self,
        project_id: Option<i64>,
    ) -> Result<Vec<WorkEdgeSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT work_edges.id, source_work_item_id, target_work_item_id
             FROM work_edges
             JOIN work_items AS source ON source.id = source_work_item_id
             JOIN work_items AS target ON target.id = target_work_item_id
             WHERE source.deleted_at_us IS NULL AND target.deleted_at_us IS NULL
               AND (? IS NULL OR (source.project_id = ? AND target.project_id = ?))
             ORDER BY work_edges.id",
        )
        .bind(project_id)
        .bind(project_id)
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(
            |(id, source_work_item_id, target_work_item_id)| WorkEdgeSummary {
                id,
                source_work_item_id,
                target_work_item_id,
            },
        )
        .collect())
    }

    pub async fn create_work_edge(
        &self,
        source_work_id: i64,
        target_work_id: i64,
    ) -> Result<(), StoreError> {
        if source_work_id == target_work_id {
            return Err(StoreError::InvalidCuration(
                "A work cannot be connected to itself.".into(),
            ));
        }
        let mut transaction = self.pool.begin().await?;
        let source_project = work_project(&mut transaction, source_work_id).await?;
        let target_project = work_project(&mut transaction, target_work_id).await?;
        if source_project != target_project {
            return Err(StoreError::InvalidCuration(
                "Work relationships cannot cross project boundaries.".into(),
            ));
        }
        sqlx::query(
            "INSERT INTO work_edges(
                 source_work_item_id, target_work_item_id, created_at_us
             ) VALUES (?, ?, ?)",
        )
        .bind(source_work_id)
        .bind(target_work_id)
        .bind(now_us()?)
        .execute(&mut *transaction)
        .await
        .map_err(|error| match error {
            sqlx::Error::Database(database) if database.is_unique_violation() => {
                StoreError::InvalidCuration("That work relationship already exists.".into())
            }
            other => StoreError::Sqlite(other),
        })?;
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn delete_work_edge(&self, edge_id: i64) -> Result<(), StoreError> {
        let mut transaction = self.pool.begin().await?;
        let removed = sqlx::query("DELETE FROM work_edges WHERE id = ?")
            .bind(edge_id)
            .execute(&mut *transaction)
            .await?
            .rows_affected();
        if removed == 0 {
            return Err(StoreError::WorkEdgeNotFound(edge_id));
        }
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        Ok(())
    }

    pub async fn work_revision(&self) -> Result<i64, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT revision FROM work_state_revision WHERE singleton = 1")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn prepare_curation(
        &self,
        project_id: i64,
        selected_activity_ids: &[i64],
    ) -> Result<CurationPreparation, StoreError> {
        validate_selected_ids(selected_activity_ids)?;
        ensure_project_exists_on_pool(&self.pool, project_id).await?;
        let mut ids = selected_activity_ids.to_vec();
        ids.sort_unstable();
        let rows = self.selected_log_rows(&ids).await?;
        if rows.len() != ids.len() {
            return Err(StoreError::InvalidCuration(
                "One or more selected logs are unavailable.".into(),
            ));
        }
        for row in &rows {
            if row.project_id != project_id {
                return Err(StoreError::InvalidCuration(
                    "Every selected log must belong to the same project.".into(),
                ));
            }
            if row.curation_state != "unreviewed" {
                return Err(StoreError::InvalidCuration(
                    "Only unreviewed logs can be proposed.".into(),
                ));
            }
        }

        let selected_text = rows
            .iter()
            .map(model_prompt_summary)
            .collect::<Vec<_>>()
            .join(" ");
        let selected_terms = tokenize(&selected_text).collect::<HashSet<_>>();
        let candidates = self.candidate_works(project_id, &selected_terms).await?;
        let mut session_groups = BTreeMap::<String, usize>::new();
        let logs = rows
            .iter()
            .enumerate()
            .map(|(index, row)| {
                let next_group = session_groups.len() + 1;
                let session_group = *session_groups
                    .entry(row.provider_session_id.clone())
                    .or_insert(next_group);
                Ok(CurationModelLog {
                    id: row.id,
                    sequence: index + 1,
                    session_group,
                    prompt_summary: model_prompt_summary(row),
                    result_summary: result_summary_from_row(row)?
                        .lines
                        .map(|lines| lines.join(" / ")),
                })
            })
            .collect::<Result<Vec<_>, StoreError>>()?;
        let input = CurationModelInput {
            project_id,
            logs,
            existing_works: candidates,
        };
        let fingerprint = curation_fingerprint(&input, &rows)?;
        let cached = self.cached_proposal(&fingerprint).await?;
        Ok(CurationPreparation {
            fingerprint,
            selected_activity_ids: ids,
            input,
            cached,
        })
    }

    pub async fn save_curation_proposal(
        &self,
        preparation: &CurationPreparation,
        groups: Vec<CurationProposalGroup>,
    ) -> Result<CurationProposal, StoreError> {
        validate_proposal_groups(
            &groups,
            preparation.selected_activity_ids(),
            &preparation
                .input
                .existing_works
                .iter()
                .map(|work| work.id)
                .collect::<BTreeSet<_>>(),
            true,
        )?;
        let selected_json = serde_json::to_string(preparation.selected_activity_ids())
            .map_err(|error| StoreError::Invariant(error.to_string()))?;
        let proposal_json = serde_json::to_string(&groups)
            .map_err(|error| StoreError::Invariant(error.to_string()))?;
        let now = now_us()?;
        let inserted = sqlx::query(
            "INSERT INTO work_curation_proposals(
                 project_id, fingerprint, selected_activity_ids_json, proposal_json,
                 state, summary_model, created_at_us, updated_at_us
             ) VALUES (?, ?, ?, ?, 'ready', ?, ?, ?)
             ON CONFLICT(fingerprint) DO NOTHING",
        )
        .bind(preparation.input.project_id)
        .bind(preparation.fingerprint())
        .bind(selected_json)
        .bind(proposal_json)
        .bind(CURATION_MODEL)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await?
        .rows_affected()
            != 0;
        let mut proposal = self
            .cached_proposal(preparation.fingerprint())
            .await?
            .ok_or_else(|| StoreError::Invariant("saved proposal is unavailable".into()))?;
        proposal.cached = !inserted;
        Ok(proposal)
    }

    pub async fn apply_curation_proposal(
        &self,
        proposal_id: i64,
        groups: Vec<CurationProposalGroup>,
    ) -> Result<CurationApplyResult, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let row = sqlx::query_as::<_, (i64, String, String)>(
            "SELECT project_id, selected_activity_ids_json, state
             FROM work_curation_proposals WHERE id = ?",
        )
        .bind(proposal_id)
        .fetch_optional(&mut *transaction)
        .await?
        .ok_or(StoreError::CurationProposalNotFound(proposal_id))?;
        if row.2 != "ready" {
            return Err(StoreError::InvalidCuration(
                "This proposal has already been applied or discarded.".into(),
            ));
        }
        let selected_ids: Vec<i64> = serde_json::from_str(&row.1)
            .map_err(|error| StoreError::Invariant(error.to_string()))?;
        let target_ids = groups
            .iter()
            .filter_map(|group| group.target_work_id)
            .collect::<BTreeSet<_>>();
        validate_proposal_groups(&groups, &selected_ids, &target_ids, false)?;
        for target_id in &target_ids {
            if work_project(&mut transaction, *target_id).await? != row.0 {
                return Err(StoreError::InvalidCuration(
                    "A proposal can only attach logs to work in the same project.".into(),
                ));
            }
        }
        ensure_selected_logs_still_available(&mut transaction, row.0, &selected_ids).await?;

        let applied_groups_json = serde_json::to_string(&groups)
            .map_err(|error| StoreError::Invariant(error.to_string()))?;
        let existing_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM work_items
             WHERE project_id = ? AND deleted_at_us IS NULL",
        )
        .bind(row.0)
        .fetch_one(&mut *transaction)
        .await?;
        let now = now_us()?;
        let mut work_ids = Vec::with_capacity(groups.len());
        let mut new_index = 0_i64;
        for group in groups {
            let title = parse_work_title(&group.title)?;
            let work_id = match group.target_work_id {
                Some(work_id) => {
                    sqlx::query("UPDATE work_items SET title = ?, updated_at_us = ? WHERE id = ?")
                        .bind(title)
                        .bind(now)
                        .bind(work_id)
                        .execute(&mut *transaction)
                        .await?;
                    work_id
                }
                None => {
                    let ordinal = existing_count + new_index;
                    new_index += 1;
                    let position_x = 96.0 + ((ordinal % 3) as f64 * 340.0);
                    let position_y = 96.0 + ((ordinal / 3) as f64 * 220.0);
                    sqlx::query(
                        "INSERT INTO work_items(
                             project_id, title, position_x, position_y,
                             created_at_us, updated_at_us
                         ) VALUES (?, ?, ?, ?, ?, ?)",
                    )
                    .bind(row.0)
                    .bind(title)
                    .bind(position_x)
                    .bind(position_y)
                    .bind(now)
                    .bind(now)
                    .execute(&mut *transaction)
                    .await?
                    .last_insert_rowid()
                }
            };
            for activity_id in group.log_ids {
                sqlx::query(
                    "INSERT INTO work_item_logs(
                         work_item_id, activity_event_id, added_via, added_at_us
                     ) VALUES (?, ?, 'ai_confirmed', ?)",
                )
                .bind(work_id)
                .bind(activity_id)
                .bind(now)
                .execute(&mut *transaction)
                .await?;
                sqlx::query("DELETE FROM activity_curation_states WHERE activity_event_id = ?")
                    .bind(activity_id)
                    .execute(&mut *transaction)
                    .await?;
            }
            work_ids.push(work_id);
        }
        sqlx::query(
            "UPDATE work_curation_proposals
             SET state = 'applied', proposal_json = ?, updated_at_us = ?
             WHERE id = ? AND state = 'ready'",
        )
        .bind(applied_groups_json)
        .bind(now)
        .bind(proposal_id)
        .execute(&mut *transaction)
        .await?;
        bump_work_revision(&mut transaction).await?;
        transaction.commit().await?;
        work_ids.sort_unstable();
        work_ids.dedup();
        Ok(CurationApplyResult { work_ids })
    }

    async fn selected_log_rows(&self, ids: &[i64]) -> Result<Vec<LogRow>, StoreError> {
        let mut builder = QueryBuilder::<Sqlite>::new(format!(
            "{LOG_SELECT}
             WHERE activity_events.activity_kind = 'user'
               AND activity_events.deleted_at_us IS NULL
               AND activity_events.id IN ("
        ));
        {
            let mut separated = builder.separated(", ");
            for id in ids {
                separated.push_bind(id);
            }
        }
        builder.push(") ORDER BY activity_events.id");
        Ok(builder
            .build_query_as::<LogRow>()
            .fetch_all(&self.pool)
            .await?)
    }

    async fn candidate_works(
        &self,
        project_id: i64,
        selected_terms: &HashSet<String>,
    ) -> Result<Vec<CurationModelWork>, StoreError> {
        let rows = sqlx::query_as::<_, CandidateRow>(
            "SELECT id, title, updated_at_us FROM work_items
             WHERE project_id = ? AND deleted_at_us IS NULL
             ORDER BY updated_at_us DESC, id DESC LIMIT 50",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await?;
        let mut scored = Vec::with_capacity(rows.len());
        for row in rows {
            let signatures = sqlx::query_scalar::<_, String>(
                "SELECT COALESCE(
                     prompt_summary.summary_text,
                     prompt_summary.projected_prompt,
                     activity_events.prompt
                 )
                 FROM work_item_logs
                 JOIN activity_events ON activity_events.id = work_item_logs.activity_event_id
                 LEFT JOIN activity_prompt_summaries AS prompt_summary
                   ON prompt_summary.activity_event_id = activity_events.id
                 WHERE work_item_logs.work_item_id = ?
                   AND activity_events.deleted_at_us IS NULL
                 ORDER BY COALESCE(
                     activity_events.captured_at_us,
                     activity_events.first_recorded_at_us
                 ) DESC, activity_events.id DESC
                 LIMIT 3",
            )
            .bind(row.id)
            .fetch_all(&self.pool)
            .await?;
            let signature = compact_chars(
                &std::iter::once(row.title.as_str())
                    .chain(signatures.iter().map(String::as_str))
                    .collect::<Vec<_>>()
                    .join(" · "),
                MAX_WORK_SIGNATURE_CHARS,
            );
            let score = tokenize(&signature)
                .filter(|term| selected_terms.contains(term))
                .count();
            scored.push((score, row.updated_at_us, row.id, row.title, signature));
        }
        scored.sort_by(|left, right| {
            right
                .0
                .cmp(&left.0)
                .then_with(|| right.1.cmp(&left.1))
                .then_with(|| right.2.cmp(&left.2))
        });
        Ok(scored
            .into_iter()
            .take(MAX_CURATION_CANDIDATES)
            .map(
                |(_, updated_at_us, id, title, signature)| CurationModelWork {
                    id,
                    title,
                    signature,
                    updated_at_us,
                },
            )
            .collect())
    }

    async fn cached_proposal(
        &self,
        fingerprint: &str,
    ) -> Result<Option<CurationProposal>, StoreError> {
        let row = sqlx::query_as::<_, (i64, i64, String, String)>(
            "SELECT id, project_id, proposal_json, summary_model
             FROM work_curation_proposals
             WHERE fingerprint = ? AND state = 'ready'",
        )
        .bind(fingerprint)
        .fetch_optional(&self.pool)
        .await?;
        row.map(|(id, project_id, proposal_json, model)| {
            let groups = serde_json::from_str(&proposal_json)
                .map_err(|error| StoreError::Invariant(error.to_string()))?;
            Ok(CurationProposal {
                id,
                project_id,
                groups,
                model,
                cached: true,
            })
        })
        .transpose()
    }

    async fn work_row(&self, work_id: i64) -> Result<WorkRow, StoreError> {
        sqlx::query_as::<_, WorkRow>(
            "SELECT work_items.id, work_items.project_id, projects.name AS project_name,
                    work_items.title, work_items.position_x, work_items.position_y,
                    work_items.updated_at_us, COUNT(work_item_logs.activity_event_id) AS log_count
             FROM work_items
             JOIN projects ON projects.id = work_items.project_id
             JOIN work_item_logs ON work_item_logs.work_item_id = work_items.id
             JOIN activity_events ON activity_events.id = work_item_logs.activity_event_id
             WHERE work_items.id = ? AND work_items.deleted_at_us IS NULL
               AND activity_events.deleted_at_us IS NULL
             GROUP BY work_items.id, projects.name",
        )
        .bind(work_id)
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::WorkNotFound(work_id))
    }

    async fn work_summary_from_row(
        &self,
        row: WorkRow,
        preview_limit: i64,
    ) -> Result<WorkItemSummary, StoreError> {
        let preview_logs = self.work_logs(row.id, preview_limit).await?;
        Ok(WorkItemSummary {
            id: row.id,
            project: ActivityProjectSummary {
                id: row.project_id,
                name: row.project_name,
            },
            title: row.title,
            log_count: row.log_count,
            position_x: row.position_x,
            position_y: row.position_y,
            updated_at_us: row.updated_at_us,
            preview_logs,
        })
    }

    async fn work_logs(&self, work_id: i64, limit: i64) -> Result<Vec<WorkLogSummary>, StoreError> {
        let rows = sqlx::query_as::<_, LogRow>(&format!(
            "{LOG_SELECT}
             WHERE work_item_logs.work_item_id = ?
               AND activity_events.deleted_at_us IS NULL
             ORDER BY COALESCE(
                 activity_events.captured_at_us,
                 activity_events.first_recorded_at_us
             ), activity_events.id
             LIMIT ?"
        ))
        .bind(work_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                let prompt_summary = prompt_summary_from_row(&row)?;
                let result_summary = result_summary_from_row(&row)?;
                Ok(WorkLogSummary {
                    id: row.id,
                    time: activity_time(row.captured_at_us, row.first_recorded_at_us)?,
                    prompt: row.prompt,
                    prompt_summary,
                    result_summary,
                })
            })
            .collect()
    }
}

const LOG_SELECT: &str = "WITH effective AS (
         SELECT activity_events.*,
                CASE
                    WHEN activity_origins.routing_mode = 'dedicated'
                    THEN activity_origins.default_project_id
                    ELSE activity_project_assignments.project_id
                END AS effective_project_id
         FROM activity_events
         LEFT JOIN activity_origins ON activity_origins.id = activity_events.origin_id
         LEFT JOIN activity_project_assignments
           ON activity_project_assignments.activity_event_id = activity_events.id
     )
     SELECT activity_events.id, effective_project_id AS project_id,
            projects.name AS project_name, activity_events.provider_session_id,
            activity_events.prompt, activity_events.captured_at_us,
            activity_events.first_recorded_at_us,
            prompt_summary.state AS prompt_summary_state,
            CASE
                WHEN prompt_summary.activity_event_id IS NULL THEN NULL
                ELSE COALESCE(prompt_summary.projected_prompt, activity_events.prompt)
            END AS projected_prompt,
            prompt_summary.summary_text AS prompt_summary_text,
            prompt_summary.used_previous_result AS prompt_summary_used_previous_result,
            prompt_summary.source_digest AS prompt_summary_source_digest,
            prompt_summary.generation AS prompt_summary_generation,
            COALESCE(result_summary.state, 'unavailable') AS result_summary_state,
            result_summary.summary_line_1, result_summary.summary_line_2,
            result_summary.summary_line_3,
            result_summary.source_text IS NOT NULL AS result_summary_source_retained,
            result_summary.source_digest AS result_summary_source_digest,
            result_summary.generation AS result_summary_generation,
            CASE
                WHEN work_item_logs.activity_event_id IS NOT NULL THEN 'organized'
                WHEN curation_state.state = 'excluded' THEN 'excluded'
                ELSE 'unreviewed'
            END AS curation_state
     FROM effective AS activity_events
     JOIN projects ON projects.id = effective_project_id
     LEFT JOIN activity_prompt_summaries AS prompt_summary
       ON prompt_summary.activity_event_id = activity_events.id
     LEFT JOIN activity_result_summaries AS result_summary
       ON result_summary.activity_event_id = activity_events.id
     LEFT JOIN work_item_logs ON work_item_logs.activity_event_id = activity_events.id
     LEFT JOIN activity_curation_states AS curation_state
       ON curation_state.activity_event_id = activity_events.id";

fn curation_log_from_row(row: LogRow) -> Result<CurationLogSummary, StoreError> {
    let state = match row.curation_state.as_str() {
        "unreviewed" => CurationLogState::Unreviewed,
        "excluded" => CurationLogState::Excluded,
        "organized" => CurationLogState::Organized,
        value => {
            return Err(StoreError::Invariant(format!(
                "invalid curation log state: {value}"
            )));
        }
    };
    let prompt_summary = prompt_summary_from_row(&row)?;
    let result_summary = result_summary_from_row(&row)?;
    Ok(CurationLogSummary {
        id: row.id,
        project: ActivityProjectSummary {
            id: row.project_id,
            name: row.project_name,
        },
        time: activity_time(row.captured_at_us, row.first_recorded_at_us)?,
        prompt: row.prompt,
        prompt_summary,
        result_summary,
        state,
    })
}

fn prompt_summary_from_row(row: &LogRow) -> Result<ActivityPromptSummary, StoreError> {
    activity_prompt_summary_from_parts(
        row.prompt_summary_state.as_deref().unwrap_or("unavailable"),
        row.projected_prompt.clone(),
        row.prompt_summary_text.clone(),
        row.prompt_summary_used_previous_result,
    )
}

fn result_summary_from_row(row: &LogRow) -> Result<ActivityResultSummary, StoreError> {
    result_summary_from_parts(
        &row.result_summary_state,
        row.summary_line_1.clone(),
        row.summary_line_2.clone(),
        row.summary_line_3.clone(),
        row.result_summary_source_retained != 0,
    )
}

fn model_prompt_summary(row: &LogRow) -> String {
    row.prompt_summary_text
        .as_deref()
        .or(row.projected_prompt.as_deref())
        .map(|value| compact_chars(value, MAX_FALLBACK_PROMPT_CHARS))
        .unwrap_or_else(|| compact_chars(&row.prompt, MAX_FALLBACK_PROMPT_CHARS))
}

fn curation_fingerprint(input: &CurationModelInput, rows: &[LogRow]) -> Result<String, StoreError> {
    let mut hasher = Sha256::new();
    hasher.update(format!("work-curation-v{CURATION_PROMPT_VERSION}\0"));
    hasher.update(
        serde_json::to_vec(input).map_err(|error| StoreError::Invariant(error.to_string()))?,
    );
    for row in rows {
        hasher.update(row.id.to_le_bytes());
        hasher.update(
            row.prompt_summary_generation
                .unwrap_or_default()
                .to_le_bytes(),
        );
        hasher.update(
            row.prompt_summary_source_digest
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
        hasher.update(
            row.result_summary_generation
                .unwrap_or_default()
                .to_le_bytes(),
        );
        hasher.update(
            row.result_summary_source_digest
                .as_deref()
                .unwrap_or("")
                .as_bytes(),
        );
    }
    for work in &input.existing_works {
        hasher.update(work.id.to_le_bytes());
        hasher.update(work.updated_at_us.to_le_bytes());
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn validate_selected_ids(ids: &[i64]) -> Result<(), StoreError> {
    if ids.is_empty() || ids.len() > MAX_CURATION_LOGS {
        return Err(StoreError::InvalidCuration(format!(
            "Select between 1 and {MAX_CURATION_LOGS} logs."
        )));
    }
    let unique = ids.iter().copied().collect::<BTreeSet<_>>();
    if unique.len() != ids.len() || unique.iter().any(|id| *id <= 0) {
        return Err(StoreError::InvalidCuration(
            "Selected log IDs must be positive and unique.".into(),
        ));
    }
    Ok(())
}

fn validate_proposal_groups(
    groups: &[CurationProposalGroup],
    selected_ids: &[i64],
    allowed_targets: &BTreeSet<i64>,
    enforce_candidate_targets: bool,
) -> Result<(), StoreError> {
    if groups.is_empty() || groups.len() > selected_ids.len() {
        return Err(StoreError::InvalidCuration(
            "A proposal must contain one or more non-empty groups.".into(),
        ));
    }
    let selected = selected_ids.iter().copied().collect::<BTreeSet<_>>();
    let mut assigned = BTreeSet::new();
    let mut used_targets = BTreeSet::new();
    for group in groups {
        parse_work_title(&group.title)?;
        if group.log_ids.is_empty() {
            return Err(StoreError::InvalidCuration(
                "A work group cannot be empty.".into(),
            ));
        }
        if let Some(target) = group.target_work_id
            && (target <= 0
                || (enforce_candidate_targets && !allowed_targets.contains(&target))
                || !used_targets.insert(target))
        {
            return Err(StoreError::InvalidCuration(
                "The proposal contains an invalid or duplicate work target.".into(),
            ));
        }
        for id in &group.log_ids {
            if !selected.contains(id) || !assigned.insert(*id) {
                return Err(StoreError::InvalidCuration(
                    "Every selected log must appear exactly once.".into(),
                ));
            }
        }
    }
    if assigned != selected {
        return Err(StoreError::InvalidCuration(
            "Every selected log must appear exactly once.".into(),
        ));
    }
    Ok(())
}

fn parse_work_title(title: &str) -> Result<&str, StoreError> {
    let trimmed = title.trim();
    let length = trimmed.chars().count();
    if trimmed != title
        || !(1..=MAX_WORK_TITLE_CHARS).contains(&length)
        || title
            .chars()
            .any(|character| matches!(character, '\r' | '\n'))
    {
        return Err(StoreError::InvalidCuration(format!(
            "Work titles must be 1 to {MAX_WORK_TITLE_CHARS} characters without outer whitespace or line breaks."
        )));
    }
    Ok(trimmed)
}

async fn ensure_project_exists_on_pool(
    pool: &sqlx::SqlitePool,
    project_id: i64,
) -> Result<(), StoreError> {
    let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_one(pool)
        .await?;
    if exists == 0 {
        return Err(StoreError::ProjectNotFound(project_id));
    }
    Ok(())
}

async fn ensure_user_activity_available(
    connection: &mut SqliteConnection,
    activity_id: i64,
) -> Result<(), StoreError> {
    let kind = sqlx::query_scalar::<_, String>(
        "SELECT activity_kind FROM activity_events
         WHERE id = ? AND deleted_at_us IS NULL",
    )
    .bind(activity_id)
    .fetch_optional(&mut *connection)
    .await?
    .ok_or(StoreError::ActivityNotFound(activity_id))?;
    if kind != "user" {
        return Err(StoreError::InvalidCuration(
            "Only user activity can be curated into work.".into(),
        ));
    }
    Ok(())
}

async fn ensure_selected_logs_still_available(
    connection: &mut SqliteConnection,
    project_id: i64,
    selected_ids: &[i64],
) -> Result<(), StoreError> {
    for id in selected_ids {
        ensure_user_activity_available(connection, *id).await?;
        let effective_project: Option<i64> = sqlx::query_scalar(
            "SELECT CASE
                 WHEN activity_origins.routing_mode = 'dedicated'
                 THEN activity_origins.default_project_id
                 ELSE activity_project_assignments.project_id
             END
             FROM activity_events
             LEFT JOIN activity_origins ON activity_origins.id = activity_events.origin_id
             LEFT JOIN activity_project_assignments
               ON activity_project_assignments.activity_event_id = activity_events.id
             WHERE activity_events.id = ?",
        )
        .bind(id)
        .fetch_one(&mut *connection)
        .await?;
        let organized: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM work_item_logs WHERE activity_event_id = ?")
                .bind(id)
                .fetch_one(&mut *connection)
                .await?;
        let excluded: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM activity_curation_states
             WHERE activity_event_id = ? AND state = 'excluded'",
        )
        .bind(id)
        .fetch_one(&mut *connection)
        .await?;
        if effective_project != Some(project_id) || organized != 0 || excluded != 0 {
            return Err(StoreError::InvalidCuration(
                "The selected logs changed after the proposal was created. Review them again."
                    .into(),
            ));
        }
    }
    Ok(())
}

async fn ensure_work_exists(
    connection: &mut SqliteConnection,
    work_id: i64,
) -> Result<(), StoreError> {
    let exists: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM work_items WHERE id = ? AND deleted_at_us IS NULL",
    )
    .bind(work_id)
    .fetch_one(&mut *connection)
    .await?;
    if exists == 0 {
        return Err(StoreError::WorkNotFound(work_id));
    }
    Ok(())
}

async fn work_project(connection: &mut SqliteConnection, work_id: i64) -> Result<i64, StoreError> {
    sqlx::query_scalar("SELECT project_id FROM work_items WHERE id = ? AND deleted_at_us IS NULL")
        .bind(work_id)
        .fetch_optional(&mut *connection)
        .await?
        .ok_or(StoreError::WorkNotFound(work_id))
}

async fn soft_delete_work_in(
    transaction: &mut Transaction<'_, Sqlite>,
    work_id: i64,
    deleted_at_us: i64,
) -> Result<(), StoreError> {
    sqlx::query(
        "DELETE FROM work_edges
         WHERE source_work_item_id = ? OR target_work_item_id = ?",
    )
    .bind(work_id)
    .bind(work_id)
    .execute(&mut **transaction)
    .await?;
    sqlx::query(
        "UPDATE work_items SET deleted_at_us = ?, updated_at_us = ?
         WHERE id = ? AND deleted_at_us IS NULL",
    )
    .bind(deleted_at_us)
    .bind(deleted_at_us)
    .bind(work_id)
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

async fn bump_work_revision(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), StoreError> {
    sqlx::query("UPDATE work_state_revision SET revision = revision + 1 WHERE singleton = 1")
        .execute(&mut **transaction)
        .await?;
    Ok(())
}

fn ensure_positive_limit(limit: i64) -> Result<(), StoreError> {
    if limit <= 0 {
        return Err(StoreError::InvalidCuration(
            "Curation page limit must be positive.".into(),
        ));
    }
    Ok(())
}

fn compact_chars(value: &str, limit: usize) -> String {
    let mut characters = value.trim().chars();
    let mut result = characters.by_ref().take(limit).collect::<String>();
    if characters.next().is_some() {
        result.pop();
        result.push('…');
    }
    result
}

fn tokenize(value: &str) -> impl Iterator<Item = String> + '_ {
    value
        .split(|character: char| !character.is_alphanumeric())
        .map(str::trim)
        .filter(|token| token.chars().count() >= 2)
        .map(str::to_lowercase)
}

fn now_us() -> Result<i64, StoreError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| StoreError::Invariant(format!("system clock is unavailable: {error}")))?
        .as_micros()
        .try_into()
        .map_err(|_| StoreError::Invariant("system clock is outside SQLite range".into()))
}

#[cfg(test)]
mod tests {
    use super::{compact_chars, parse_work_title, tokenize};

    #[test]
    fn compact_text_and_terms_are_bounded_and_deterministic() {
        assert_eq!(compact_chars("abcdef", 4), "abc…");
        assert_eq!(
            tokenize("Windows portable 배포").collect::<Vec<_>>(),
            ["windows", "portable", "배포",]
        );
    }

    #[test]
    fn work_title_rejects_whitespace_and_line_breaks() {
        assert!(parse_work_title("Portable 배포").is_ok());
        assert!(parse_work_title(" Portable 배포").is_err());
        assert!(parse_work_title("Portable\n배포").is_err());
    }
}
