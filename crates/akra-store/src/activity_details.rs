use akra_core::ingress::ActivityKind;

use crate::{
    ActivityConversationTurn, ActivityDetail, ActivityKindFilter, ActivityOriginDetail,
    ActivityResultSummary, ActivityStore, ActivityTechnicalDetail, ResultSummaryLines,
    ResultSummaryStatus, StoreError,
    activities::{activity_time, explicit_activity_time, project_summary, result_summary_status},
    prompt_summaries::activity_prompt_summary_from_parts,
};

#[derive(sqlx::FromRow)]
struct DetailRow {
    id: i64,
    provider: String,
    provider_session_id: String,
    provider_turn_id: String,
    activity_kind: String,
    agent_id: Option<String>,
    agent_type: Option<String>,
    prompt: String,
    submitted_cwd: Option<String>,
    captured_at_us: Option<i64>,
    captured_at_provenance: Option<String>,
    first_recorded_at_us: Option<i64>,
    first_recorded_at_provenance: Option<String>,
    origin_id: i64,
    origin_kind: String,
    origin_resolution_source: String,
    origin_display_path: String,
    origin_activity_count: i64,
    conversation_total: i64,
    project_id: Option<i64>,
    project_name: Option<String>,
    on_canvas: i64,
    result_summary_state: String,
    summary_line_1: Option<String>,
    summary_line_2: Option<String>,
    summary_line_3: Option<String>,
    prompt_summary_state: Option<String>,
    projected_prompt: Option<String>,
    prompt_summary_text: Option<String>,
    prompt_summary_used_previous_result: Option<i64>,
}

#[derive(sqlx::FromRow)]
struct TimelineRow {
    id: i64,
    activity_kind: String,
    prompt: String,
    project_id: Option<i64>,
    project_name: Option<String>,
    captured_at_us: Option<i64>,
    first_recorded_at_us: Option<i64>,
    on_canvas: i64,
    result_summary_state: String,
    summary_line_1: Option<String>,
    summary_line_2: Option<String>,
    summary_line_3: Option<String>,
    prompt_summary_state: Option<String>,
    projected_prompt: Option<String>,
    prompt_summary_text: Option<String>,
    prompt_summary_used_previous_result: Option<i64>,
}

impl ActivityStore {
    pub async fn activity_detail(&self, activity_id: i64) -> Result<ActivityDetail, StoreError> {
        self.activity_detail_page(activity_id, None, i64::MAX).await
    }

    pub async fn activity_detail_page(
        &self,
        activity_id: i64,
        conversation_after_id: Option<i64>,
        conversation_limit: i64,
    ) -> Result<ActivityDetail, StoreError> {
        self.activity_detail_page_filtered(
            activity_id,
            conversation_after_id,
            conversation_limit,
            ActivityKindFilter::ALL,
        )
        .await
    }

    pub async fn activity_detail_page_filtered(
        &self,
        activity_id: i64,
        conversation_after_id: Option<i64>,
        conversation_limit: i64,
        activity_filter: ActivityKindFilter,
    ) -> Result<ActivityDetail, StoreError> {
        let row = sqlx::query_as::<_, DetailRow>(
            "WITH selected AS (
                 SELECT activity_events.*,
                        activity_origins.kind AS origin_kind,
                        activity_origins.resolution_source AS origin_resolution_source,
                        activity_origins.display_path AS origin_display_path,
                        (
                            SELECT COUNT(*) FROM activity_events AS sibling
                            WHERE sibling.origin_id = activity_events.origin_id
                        ) AS origin_activity_count,
                        CASE
                            WHEN activity_origins.routing_mode = 'dedicated'
                            THEN activity_origins.default_project_id
                            ELSE activity_project_assignments.project_id
                        END AS project_id
                 FROM activity_events
                 JOIN activity_origins ON activity_origins.id = activity_events.origin_id
                 LEFT JOIN activity_project_assignments
                   ON activity_project_assignments.activity_event_id = activity_events.id
                 WHERE activity_events.id = ?
             )
             SELECT selected.id, selected.provider, selected.provider_session_id,
                    selected.provider_turn_id, selected.activity_kind,
                    selected.agent_id, selected.agent_type,
                    selected.prompt, selected.submitted_cwd,
                    selected.captured_at_us, selected.captured_at_provenance,
                    selected.first_recorded_at_us, selected.first_recorded_at_provenance,
                    selected.origin_id, selected.origin_kind, selected.origin_resolution_source,
                    selected.origin_display_path, selected.origin_activity_count,
                    (
                        SELECT COUNT(*) FROM activity_events AS turn
                        WHERE turn.provider = selected.provider
                          AND turn.provider_session_id = selected.provider_session_id
                          AND (
                                 turn.activity_kind = 'user'
                              OR (?2 = 1 AND turn.activity_kind = 'subagent')
                              OR (?3 = 1 AND turn.activity_kind = 'internal')
                          )
                    ) AS conversation_total,
                    selected.project_id, projects.name AS project_name,
                    EXISTS (
                        SELECT 1 FROM canvas_nodes
                        WHERE canvas_nodes.activity_event_id = selected.id
                          AND canvas_nodes.deleted_at_us IS NULL
                    ) AS on_canvas,
                    COALESCE(result_summary.state, 'unavailable') AS result_summary_state,
                    result_summary.summary_line_1,
                    result_summary.summary_line_2,
                    result_summary.summary_line_3,
                    prompt_summary.state AS prompt_summary_state,
                    prompt_summary.projected_prompt,
                    prompt_summary.summary_text AS prompt_summary_text,
                    prompt_summary.used_previous_result AS prompt_summary_used_previous_result
             FROM selected
             LEFT JOIN projects ON projects.id = selected.project_id
             LEFT JOIN activity_result_summaries AS result_summary
               ON result_summary.activity_event_id = selected.id
             LEFT JOIN activity_prompt_summaries AS prompt_summary
               ON prompt_summary.activity_event_id = selected.id",
        )
        .bind(activity_id)
        .bind(activity_filter.include_subagent())
        .bind(activity_filter.include_internal())
        .fetch_optional(&self.pool)
        .await?
        .ok_or(StoreError::ActivityNotFound(activity_id))?;
        let mut conversation = self
            .conversation(
                &row.provider,
                &row.provider_session_id,
                activity_id,
                conversation_after_id,
                conversation_limit.saturating_add(1),
                activity_filter,
            )
            .await?;
        let conversation_has_more =
            i64::try_from(conversation.len()).is_ok_and(|length| length > conversation_limit);
        if conversation_has_more {
            conversation.pop();
        }
        let result_summary = result_summary_from_parts(
            &row.result_summary_state,
            row.summary_line_1,
            row.summary_line_2,
            row.summary_line_3,
        )?;
        let prompt_summary = activity_prompt_summary_from_parts(
            row.prompt_summary_state.as_deref().unwrap_or("unavailable"),
            row.projected_prompt.clone(),
            row.prompt_summary_text.clone(),
            row.prompt_summary_used_previous_result,
        )?;
        let selected_turn = ActivityConversationTurn {
            id: row.id,
            activity_kind: parse_activity_kind(&row.activity_kind)?,
            prompt: row.prompt.clone(),
            project: project_summary(row.project_id, row.project_name.clone())?,
            time: activity_time(row.captured_at_us, row.first_recorded_at_us)?,
            on_canvas: row.on_canvas != 0,
            selected: true,
            result_summary: result_summary.clone(),
            prompt_summary: prompt_summary.clone(),
        };
        Ok(ActivityDetail {
            id: row.id,
            provider: row.provider,
            activity_kind: parse_activity_kind(&row.activity_kind)?,
            prompt: row.prompt,
            project: project_summary(row.project_id, row.project_name)?,
            captured_at: explicit_activity_time(
                row.captured_at_us,
                row.captured_at_provenance.as_deref(),
            )?,
            first_recorded_at: explicit_activity_time(
                row.first_recorded_at_us,
                row.first_recorded_at_provenance.as_deref(),
            )?,
            on_canvas: row.on_canvas != 0,
            submitted_cwd: row.submitted_cwd,
            origin: ActivityOriginDetail {
                id: row.origin_id,
                kind: row.origin_kind,
                resolution_source: row.origin_resolution_source,
                display_path: row.origin_display_path,
                activity_count: row.origin_activity_count,
            },
            technical: ActivityTechnicalDetail {
                session_id: row.provider_session_id,
                turn_id: row.provider_turn_id,
                agent_id: row.agent_id,
                agent_type: row.agent_type,
            },
            result_summary,
            prompt_summary,
            selected_turn,
            conversation,
            conversation_total: row.conversation_total,
            conversation_has_more,
        })
    }

    async fn conversation(
        &self,
        provider: &str,
        session_id: &str,
        selected_id: i64,
        after_id: Option<i64>,
        fetch_limit: i64,
        activity_filter: ActivityKindFilter,
    ) -> Result<Vec<ActivityConversationTurn>, StoreError> {
        let rows = sqlx::query_as::<_, TimelineRow>(
            "WITH effective AS (
                 SELECT activity_events.id, activity_events.activity_kind,
                        activity_events.prompt,
                        activity_events.captured_at_us,
                        activity_events.first_recorded_at_us,
                        COALESCE(result_summary.state, 'unavailable') AS result_summary_state,
                        result_summary.summary_line_1,
                        result_summary.summary_line_2,
                        result_summary.summary_line_3,
                        prompt_summary.state AS prompt_summary_state,
                        prompt_summary.projected_prompt,
                        prompt_summary.summary_text AS prompt_summary_text,
                        prompt_summary.used_previous_result AS prompt_summary_used_previous_result,
                        CASE
                            WHEN activity_origins.routing_mode = 'dedicated'
                            THEN activity_origins.default_project_id
                            ELSE activity_project_assignments.project_id
                        END AS project_id
                 FROM activity_events
                 JOIN activity_origins ON activity_origins.id = activity_events.origin_id
                 LEFT JOIN activity_project_assignments
                   ON activity_project_assignments.activity_event_id = activity_events.id
                 LEFT JOIN activity_result_summaries AS result_summary
                   ON result_summary.activity_event_id = activity_events.id
                 LEFT JOIN activity_prompt_summaries AS prompt_summary
                   ON prompt_summary.activity_event_id = activity_events.id
                 WHERE activity_events.provider = ?1
                   AND activity_events.provider_session_id = ?2
                   AND (
                          activity_events.activity_kind = 'user'
                       OR (?3 = 1 AND activity_events.activity_kind = 'subagent')
                       OR (?4 = 1 AND activity_events.activity_kind = 'internal')
                   )
             ),
             classified AS (
                 SELECT effective.*, projects.name AS project_name,
                        CASE
                            WHEN COALESCE(captured_at_us, first_recorded_at_us) IS NULL THEN 1
                            ELSE 0
                        END AS time_class,
                        COALESCE(captured_at_us, first_recorded_at_us) AS ordered_at_us,
                        EXISTS (
                            SELECT 1 FROM canvas_nodes
                            WHERE canvas_nodes.activity_event_id = effective.id
                              AND canvas_nodes.deleted_at_us IS NULL
                        ) AS on_canvas
                 FROM effective
                 LEFT JOIN projects ON projects.id = effective.project_id
             ),
             numbered AS (
                 SELECT classified.*,
                        ROW_NUMBER() OVER (
                            ORDER BY time_class, ordered_at_us, id
                        ) AS conversation_index
                 FROM classified
             ),
             cursor AS (
                 SELECT conversation_index FROM numbered WHERE id = ?5
             )
             SELECT id, activity_kind, prompt, project_id, project_name,
                    captured_at_us, first_recorded_at_us, on_canvas,
                    result_summary_state, summary_line_1, summary_line_2, summary_line_3
                    , prompt_summary_state, projected_prompt, prompt_summary_text,
                    prompt_summary_used_previous_result
             FROM numbered
             WHERE ?5 IS NULL
                OR conversation_index > COALESCE(
                    (SELECT conversation_index FROM cursor),
                    9223372036854775807
                )
             ORDER BY conversation_index
             LIMIT ?6",
        )
        .bind(provider)
        .bind(session_id)
        .bind(activity_filter.include_subagent())
        .bind(activity_filter.include_internal())
        .bind(after_id)
        .bind(fetch_limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter()
            .map(|row| {
                Ok(ActivityConversationTurn {
                    id: row.id,
                    activity_kind: parse_activity_kind(&row.activity_kind)?,
                    prompt: row.prompt,
                    project: project_summary(row.project_id, row.project_name)?,
                    time: activity_time(row.captured_at_us, row.first_recorded_at_us)?,
                    on_canvas: row.on_canvas != 0,
                    selected: row.id == selected_id,
                    result_summary: result_summary_from_parts(
                        &row.result_summary_state,
                        row.summary_line_1,
                        row.summary_line_2,
                        row.summary_line_3,
                    )?,
                    prompt_summary: activity_prompt_summary_from_parts(
                        row.prompt_summary_state.as_deref().unwrap_or("unavailable"),
                        row.projected_prompt,
                        row.prompt_summary_text,
                        row.prompt_summary_used_previous_result,
                    )?,
                })
            })
            .collect()
    }
}

fn result_summary_from_parts(
    state: &str,
    line_1: Option<String>,
    line_2: Option<String>,
    line_3: Option<String>,
) -> Result<ActivityResultSummary, StoreError> {
    let status = result_summary_status(state)?;
    let lines = match (status, line_1, line_2, line_3) {
        (ResultSummaryStatus::Ready, Some(line_1), Some(line_2), Some(line_3)) => {
            Some(ResultSummaryLines::try_new(line_1, line_2, line_3)?.into_array())
        }
        (ResultSummaryStatus::Ready, _, _, _) => {
            return Err(StoreError::Invariant(
                "ready result summary is missing one or more lines".into(),
            ));
        }
        _ => None,
    };
    Ok(ActivityResultSummary { status, lines })
}

fn parse_activity_kind(value: &str) -> Result<ActivityKind, StoreError> {
    ActivityKind::from_storage(value)
        .ok_or_else(|| StoreError::Invariant(format!("invalid activity kind: {value}")))
}
