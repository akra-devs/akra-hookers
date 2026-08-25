use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use akra_core::ingress::ActivityKind;

use crate::{
    ActivityProjectSummary, ActivityStore, ActivitySummary, ActivityTimeProvenance,
    ActivityTimeSummary, ResultSummaryStatus, StoreError,
    prompt_summaries::activity_prompt_summary_from_parts,
};

const MAX_SUMMARY_PROMPT_CHARS: usize = 280;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityScope {
    All,
    Inbox,
    Project(i64),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ActivityOrder {
    Oldest,
    Newest,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityKindFilter {
    include_subagent: bool,
    include_internal: bool,
}

impl ActivityKindFilter {
    pub const ALL: Self = Self::new(true, true);

    pub const fn new(include_subagent: bool, include_internal: bool) -> Self {
        Self {
            include_subagent,
            include_internal,
        }
    }

    pub(crate) const fn include_subagent(self) -> i64 {
        self.include_subagent as i64
    }

    pub(crate) const fn include_internal(self) -> i64 {
        self.include_internal as i64
    }
}

impl Default for ActivityKindFilter {
    fn default() -> Self {
        Self::ALL
    }
}

/// An inclusive lower time bound for activity-derived views.
///
/// Activities without a captured or recorded timestamp remain visible in the
/// all-time view, but intentionally do not masquerade as recent activity.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ActivityTimeRange {
    start_at_us: Option<i64>,
}

impl ActivityTimeRange {
    pub const ALL: Self = Self { start_at_us: None };

    pub const fn since(start_at_us: i64) -> Self {
        Self {
            start_at_us: Some(start_at_us),
        }
    }

    pub(crate) const fn start_at_us(self) -> Option<i64> {
        self.start_at_us
    }
}

impl Default for ActivityTimeRange {
    fn default() -> Self {
        Self::ALL
    }
}

type SummaryRow = (
    i64,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    i64,
    i64,
    String,
    Option<String>,
    Option<String>,
    Option<String>,
    Option<i64>,
);

impl ActivityStore {
    pub async fn activity_count(&self) -> Result<i64, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT COUNT(*) FROM activity_events WHERE deleted_at_us IS NULL")
                .fetch_one(&self.pool)
                .await?,
        )
    }

    pub async fn activities(&self) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activity_summaries(ActivityScope::All).await
    }

    pub async fn activity_summaries(
        &self,
        scope: ActivityScope,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activity_summaries_page(scope, None, i64::MAX).await
    }

    pub async fn activity_summaries_page(
        &self,
        scope: ActivityScope,
        after_id: Option<i64>,
        limit: i64,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activity_summaries_ordered_page(scope, after_id, limit, ActivityOrder::Oldest)
            .await
    }

    pub async fn activity_summaries_ordered_page(
        &self,
        scope: ActivityScope,
        cursor_id: Option<i64>,
        limit: i64,
        order: ActivityOrder,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        let (scope_name, project_id) = match scope {
            ActivityScope::All => ("all", None),
            ActivityScope::Inbox => ("inbox", None),
            ActivityScope::Project(project_id) => {
                let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
                    .bind(project_id)
                    .fetch_one(&self.pool)
                    .await?;
                if exists == 0 {
                    return Err(StoreError::ProjectNotFound(project_id));
                }
                ("project", Some(project_id))
            }
        };
        let order_name = match order {
            ActivityOrder::Oldest => "oldest",
            ActivityOrder::Newest => "newest",
        };
        let rows = sqlx::query_as::<_, SummaryRow>(
            "WITH effective AS (
                 SELECT activity_events.id, activity_events.provider,
                        activity_events.activity_kind,
                        activity_events.provider_session_id, activity_events.prompt,
                        activity_events.captured_at_us,
                        activity_events.first_recorded_at_us,
                        activity_events.global_sequence,
                        CASE
                            WHEN activity_origins.routing_mode = 'dedicated'
                            THEN activity_origins.default_project_id
                            ELSE activity_project_assignments.project_id
                        END AS effective_project_id
                 FROM activity_events
                 LEFT JOIN activity_origins
                   ON activity_origins.id = activity_events.origin_id
                 LEFT JOIN activity_project_assignments
                   ON activity_project_assignments.activity_event_id = activity_events.id
                 WHERE activity_events.deleted_at_us IS NULL
             ),
             classified AS (
                 SELECT effective.*, projects.name AS project_name,
                        CASE
                            WHEN COALESCE(captured_at_us, first_recorded_at_us) IS NULL THEN 1
                            ELSE 0
                        END AS time_class,
                        COALESCE(captured_at_us, first_recorded_at_us) AS ordered_at_us
                 FROM effective
                 LEFT JOIN projects ON projects.id = effective.effective_project_id
             ),
             numbered AS (
                 SELECT classified.*,
                        LAG(id) OVER (
                            PARTITION BY provider, provider_session_id
                            ORDER BY time_class, ordered_at_us, id
                        ) AS previous_conversation_activity_id,
                        ROW_NUMBER() OVER (
                            PARTITION BY provider, provider_session_id
                            ORDER BY time_class, ordered_at_us, id
                        ) AS conversation_index,
                        COUNT(*) OVER (
                            PARTITION BY provider, provider_session_id
                        ) AS conversation_total
                 FROM classified
             ),
             cursor AS (
                 SELECT global_sequence, id
                 FROM activity_events
                 WHERE id = ?
             )
             SELECT id, provider, activity_kind, prompt, effective_project_id, project_name,
                     captured_at_us, first_recorded_at_us,
                     previous_conversation_activity_id,
                     conversation_index, conversation_total,
                    COALESCE(
                        (
                            SELECT state FROM activity_result_summaries
                            WHERE activity_event_id = numbered.id
                        ),
                        'unavailable'
                    ) AS result_summary_state,
                    prompt_summary.state AS prompt_summary_state,
                    CASE
                        WHEN prompt_summary.activity_event_id IS NULL THEN NULL
                        ELSE COALESCE(prompt_summary.projected_prompt, numbered.prompt)
                    END AS projected_prompt,
                    prompt_summary.summary_text,
                    prompt_summary.used_previous_result
             FROM numbered
             LEFT JOIN activity_prompt_summaries AS prompt_summary
               ON prompt_summary.activity_event_id = numbered.id
             WHERE (
                    (? = 'all')
                 OR (? = 'inbox' AND effective_project_id IS NULL)
                 OR (? = 'project' AND effective_project_id = ?)
             )
             AND (
                 ? IS NULL
                 OR EXISTS (
                     SELECT 1 FROM cursor
                     WHERE (
                         ? = 'oldest'
                         AND (
                             (
                                 cursor.global_sequence IS NOT NULL
                                 AND (
                                     numbered.global_sequence IS NULL
                                     OR numbered.global_sequence > cursor.global_sequence
                                     OR (
                                         numbered.global_sequence = cursor.global_sequence
                                         AND numbered.id > cursor.id
                                     )
                                 )
                             )
                             OR (
                                 cursor.global_sequence IS NULL
                                 AND numbered.global_sequence IS NULL
                                 AND numbered.id > cursor.id
                             )
                         )
                     )
                     OR (
                         ? = 'newest'
                         AND (
                             (
                                 cursor.global_sequence IS NOT NULL
                                 AND (
                                     numbered.global_sequence IS NULL
                                     OR numbered.global_sequence < cursor.global_sequence
                                     OR (
                                         numbered.global_sequence = cursor.global_sequence
                                         AND numbered.id < cursor.id
                                     )
                                 )
                             )
                             OR (
                                 cursor.global_sequence IS NULL
                                 AND numbered.global_sequence IS NULL
                                 AND numbered.id < cursor.id
                             )
                         )
                     )
                 )
             )
             ORDER BY global_sequence IS NULL,
                      CASE WHEN ? = 'oldest' THEN global_sequence END,
                      CASE WHEN ? = 'newest' THEN global_sequence END DESC,
                      CASE WHEN ? = 'oldest' THEN id END,
                      CASE WHEN ? = 'newest' THEN id END DESC
             LIMIT ?",
        )
        .bind(cursor_id)
        .bind(scope_name)
        .bind(scope_name)
        .bind(scope_name)
        .bind(project_id)
        .bind(cursor_id)
        .bind(order_name)
        .bind(order_name)
        .bind(order_name)
        .bind(order_name)
        .bind(order_name)
        .bind(order_name)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(summary_from_row).collect()
    }

    pub async fn activity_summaries_indexed_page(
        &self,
        scope: ActivityScope,
        cursor_id: Option<i64>,
        limit: i64,
        order: ActivityOrder,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activity_summaries_indexed_page_filtered(
            scope,
            cursor_id,
            limit,
            order,
            ActivityKindFilter::ALL,
        )
        .await
    }

    pub async fn activity_summaries_indexed_page_filtered(
        &self,
        scope: ActivityScope,
        cursor_id: Option<i64>,
        limit: i64,
        order: ActivityOrder,
        activity_filter: ActivityKindFilter,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activity_summaries_indexed_page_filtered_in_range(
            scope,
            cursor_id,
            limit,
            order,
            activity_filter,
            ActivityTimeRange::ALL,
        )
        .await
    }

    pub async fn activity_summaries_indexed_page_filtered_in_range(
        &self,
        scope: ActivityScope,
        cursor_id: Option<i64>,
        limit: i64,
        order: ActivityOrder,
        activity_filter: ActivityKindFilter,
        time_range: ActivityTimeRange,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        let (scope_name, project_id) = match scope {
            ActivityScope::All => ("all", None),
            ActivityScope::Inbox => ("inbox", None),
            ActivityScope::Project(project_id) => {
                let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
                    .bind(project_id)
                    .fetch_one(&self.pool)
                    .await?;
                if exists == 0 {
                    return Err(StoreError::ProjectNotFound(project_id));
                }
                ("project", Some(project_id))
            }
        };
        let order_name = match order {
            ActivityOrder::Oldest => "oldest",
            ActivityOrder::Newest => "newest",
        };
        let rows = sqlx::query_as::<_, SummaryRow>(
            "WITH cursor AS (
                 SELECT global_sequence, id FROM activity_events WHERE id = ?1
             ),
             effective AS (
                 SELECT activity_events.id, activity_events.provider,
                        activity_events.activity_kind,
                        activity_events.provider_session_id,
                        activity_events.prompt,
                        activity_events.captured_at_us,
                        activity_events.first_recorded_at_us,
                        activity_events.global_sequence,
                        CASE
                            WHEN activity_origins.routing_mode = 'dedicated'
                            THEN activity_origins.default_project_id
                            ELSE activity_project_assignments.project_id
                        END AS effective_project_id
                 FROM activity_events
                 LEFT JOIN activity_origins
                   ON activity_origins.id = activity_events.origin_id
                 LEFT JOIN activity_project_assignments
                   ON activity_project_assignments.activity_event_id = activity_events.id
                 WHERE activity_events.deleted_at_us IS NULL
             ),
             sequenced AS MATERIALIZED (
                 SELECT effective.*,
                        LAG(id) OVER (
                            PARTITION BY provider, provider_session_id
                            ORDER BY
                                COALESCE(captured_at_us, first_recorded_at_us) IS NULL,
                                COALESCE(captured_at_us, first_recorded_at_us),
                                id
                        ) AS previous_conversation_activity_id
                 FROM effective
                 WHERE effective.activity_kind = 'user'
                    OR (?6 = 1 AND effective.activity_kind = 'subagent')
                    OR (?7 = 1 AND effective.activity_kind = 'internal')
             ),
             page AS MATERIALIZED (
                 SELECT sequenced.*
                 FROM sequenced
                 WHERE (
                        (?2 = 'all')
                     OR (?2 = 'inbox' AND effective_project_id IS NULL)
                     OR (?2 = 'project' AND effective_project_id = ?3)
                 )
                 AND (
                     ?8 IS NULL
                     OR COALESCE(
                         sequenced.captured_at_us,
                         sequenced.first_recorded_at_us
                     ) >= ?8
                 )
                 AND (
                     ?1 IS NULL
                     OR EXISTS (
                         SELECT 1 FROM cursor
                         WHERE (
                             ?4 = 'oldest'
                             AND (
                                 (
                                     cursor.global_sequence IS NOT NULL
                                     AND (
                                         sequenced.global_sequence IS NULL
                                         OR sequenced.global_sequence > cursor.global_sequence
                                         OR (
                                             sequenced.global_sequence = cursor.global_sequence
                                             AND sequenced.id > cursor.id
                                         )
                                     )
                                 )
                                 OR (
                                     cursor.global_sequence IS NULL
                                     AND sequenced.global_sequence IS NULL
                                     AND sequenced.id > cursor.id
                                 )
                             )
                         )
                         OR (
                             ?4 = 'newest'
                             AND (
                                 (
                                     cursor.global_sequence IS NOT NULL
                                     AND (
                                         sequenced.global_sequence IS NULL
                                         OR sequenced.global_sequence < cursor.global_sequence
                                         OR (
                                             sequenced.global_sequence = cursor.global_sequence
                                             AND sequenced.id < cursor.id
                                         )
                                     )
                                 )
                                 OR (
                                     cursor.global_sequence IS NULL
                                     AND sequenced.global_sequence IS NULL
                                     AND sequenced.id < cursor.id
                                 )
                             )
                         )
                     )
                 )
                 ORDER BY global_sequence IS NULL,
                          CASE WHEN ?4 = 'oldest' THEN global_sequence END,
                          CASE WHEN ?4 = 'newest' THEN global_sequence END DESC,
                          CASE WHEN ?4 = 'oldest' THEN id END,
                          CASE WHEN ?4 = 'newest' THEN id END DESC
                 LIMIT ?5
             )
             SELECT page.id, page.provider, page.activity_kind, page.prompt,
                     page.effective_project_id, projects.name,
                     page.captured_at_us, page.first_recorded_at_us,
                     page.previous_conversation_activity_id,
                     (
                        SELECT COUNT(*) FROM activity_events AS turn
                         WHERE turn.provider = page.provider
                           AND turn.provider_session_id = page.provider_session_id
                           AND turn.deleted_at_us IS NULL
                          AND (
                                 turn.activity_kind = 'user'
                              OR (?6 = 1 AND turn.activity_kind = 'subagent')
                              OR (?7 = 1 AND turn.activity_kind = 'internal')
                          )
                          AND (
                              (
                                  COALESCE(page.captured_at_us, page.first_recorded_at_us) IS NULL
                                  AND (
                                      COALESCE(turn.captured_at_us, turn.first_recorded_at_us)
                                          IS NOT NULL
                                      OR (
                                          COALESCE(
                                              turn.captured_at_us,
                                              turn.first_recorded_at_us
                                          ) IS NULL
                                          AND turn.id <= page.id
                                      )
                                  )
                              )
                              OR (
                                  COALESCE(page.captured_at_us, page.first_recorded_at_us)
                                      IS NOT NULL
                                  AND COALESCE(
                                      turn.captured_at_us,
                                      turn.first_recorded_at_us
                                  ) IS NOT NULL
                                  AND (
                                      COALESCE(
                                          turn.captured_at_us,
                                          turn.first_recorded_at_us
                                      ) < COALESCE(
                                          page.captured_at_us,
                                          page.first_recorded_at_us
                                      )
                                      OR (
                                          COALESCE(
                                              turn.captured_at_us,
                                              turn.first_recorded_at_us
                                          ) = COALESCE(
                                              page.captured_at_us,
                                              page.first_recorded_at_us
                                          )
                                          AND turn.id <= page.id
                                      )
                                  )
                              )
                          )
                    ) AS conversation_index,
                    (
                        SELECT COUNT(*) FROM activity_events AS turn
                         WHERE turn.provider = page.provider
                           AND turn.provider_session_id = page.provider_session_id
                           AND turn.deleted_at_us IS NULL
                          AND (
                                 turn.activity_kind = 'user'
                              OR (?6 = 1 AND turn.activity_kind = 'subagent')
                              OR (?7 = 1 AND turn.activity_kind = 'internal')
                          )
                    ) AS conversation_total,
                    COALESCE(
                        (
                            SELECT state FROM activity_result_summaries
                            WHERE activity_event_id = page.id
                        ),
                        'unavailable'
                    ) AS result_summary_state,
                    prompt_summary.state AS prompt_summary_state,
                    CASE
                        WHEN prompt_summary.activity_event_id IS NULL THEN NULL
                        ELSE COALESCE(prompt_summary.projected_prompt, page.prompt)
                    END AS projected_prompt,
                    prompt_summary.summary_text,
                    prompt_summary.used_previous_result
             FROM page
             LEFT JOIN projects ON projects.id = page.effective_project_id
             LEFT JOIN activity_prompt_summaries AS prompt_summary
               ON prompt_summary.activity_event_id = page.id
             ORDER BY page.global_sequence IS NULL,
                      CASE WHEN ?4 = 'oldest' THEN page.global_sequence END,
                      CASE WHEN ?4 = 'newest' THEN page.global_sequence END DESC,
                      CASE WHEN ?4 = 'oldest' THEN page.id END,
                      CASE WHEN ?4 = 'newest' THEN page.id END DESC",
        )
        .bind(cursor_id)
        .bind(scope_name)
        .bind(project_id)
        .bind(order_name)
        .bind(limit)
        .bind(activity_filter.include_subagent())
        .bind(activity_filter.include_internal())
        .bind(time_range.start_at_us())
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(summary_from_row).collect()
    }

    pub async fn activity_summary_count(&self, scope: ActivityScope) -> Result<i64, StoreError> {
        self.activity_summary_count_filtered(scope, ActivityKindFilter::ALL)
            .await
    }

    pub async fn activity_summary_count_filtered(
        &self,
        scope: ActivityScope,
        activity_filter: ActivityKindFilter,
    ) -> Result<i64, StoreError> {
        self.activity_summary_count_filtered_in_range(
            scope,
            activity_filter,
            ActivityTimeRange::ALL,
        )
        .await
    }

    pub async fn activity_summary_count_filtered_in_range(
        &self,
        scope: ActivityScope,
        activity_filter: ActivityKindFilter,
        time_range: ActivityTimeRange,
    ) -> Result<i64, StoreError> {
        let (scope_name, project_id) = match scope {
            ActivityScope::All => ("all", None),
            ActivityScope::Inbox => ("inbox", None),
            ActivityScope::Project(project_id) => {
                let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
                    .bind(project_id)
                    .fetch_one(&self.pool)
                    .await?;
                if exists == 0 {
                    return Err(StoreError::ProjectNotFound(project_id));
                }
                ("project", Some(project_id))
            }
        };
        Ok(sqlx::query_scalar(
            "WITH effective AS (
                 SELECT activity_events.activity_kind,
                        activity_events.captured_at_us,
                        activity_events.first_recorded_at_us,
                        CASE
                            WHEN activity_origins.routing_mode = 'dedicated'
                            THEN activity_origins.default_project_id
                            ELSE activity_project_assignments.project_id
                        END AS effective_project_id
                 FROM activity_events
                 LEFT JOIN activity_origins
                   ON activity_origins.id = activity_events.origin_id
                 LEFT JOIN activity_project_assignments
                   ON activity_project_assignments.activity_event_id = activity_events.id
                 WHERE activity_events.deleted_at_us IS NULL
             )
             SELECT COUNT(*) FROM effective
             WHERE (
                    (?1 = 'all')
                 OR (?2 = 'inbox' AND effective_project_id IS NULL)
                 OR (?3 = 'project' AND effective_project_id = ?4)
             )
            AND (
                    activity_kind = 'user'
                 OR (?5 = 1 AND activity_kind = 'subagent')
                 OR (?6 = 1 AND activity_kind = 'internal')
            )
            AND (
                ?7 IS NULL
                OR COALESCE(captured_at_us, first_recorded_at_us) >= ?7
            )",
        )
        .bind(scope_name)
        .bind(scope_name)
        .bind(scope_name)
        .bind(project_id)
        .bind(activity_filter.include_subagent())
        .bind(activity_filter.include_internal())
        .bind(time_range.start_at_us())
        .fetch_one(&self.pool)
        .await?)
    }
}

fn summary_from_row(row: SummaryRow) -> Result<ActivitySummary, StoreError> {
    let (
        id,
        provider,
        activity_kind,
        prompt,
        project_id,
        project_name,
        captured_at_us,
        first_recorded_at_us,
        previous_conversation_activity_id,
        conversation_index,
        conversation_total,
        result_summary_state,
        prompt_summary_state,
        projected_prompt,
        prompt_summary_text,
        used_previous_result,
    ) = row;
    let project = project_summary(project_id, project_name)?;
    Ok(ActivitySummary {
        id,
        provider,
        activity_kind: ActivityKind::from_storage(&activity_kind).ok_or_else(|| {
            StoreError::Invariant(format!("invalid activity kind: {activity_kind}"))
        })?,
        prompt: prompt_preview(&prompt),
        project,
        time: activity_time(captured_at_us, first_recorded_at_us)?,
        previous_conversation_activity_id,
        conversation_index,
        conversation_total,
        result_summary_status: result_summary_status(&result_summary_state)?,
        prompt_summary: activity_prompt_summary_from_parts(
            prompt_summary_state.as_deref().unwrap_or("unavailable"),
            projected_prompt,
            prompt_summary_text,
            used_previous_result,
        )?,
    })
}

pub(crate) fn result_summary_status(value: &str) -> Result<ResultSummaryStatus, StoreError> {
    match value {
        "pending" | "running" | "retry_wait" => Ok(ResultSummaryStatus::Pending),
        "succeeded" => Ok(ResultSummaryStatus::Ready),
        "failed" => Ok(ResultSummaryStatus::Failed),
        "skipped" | "unavailable" => Ok(ResultSummaryStatus::Unavailable),
        _ => Err(StoreError::Invariant(format!(
            "invalid result summary state: {value}"
        ))),
    }
}

fn prompt_preview(prompt: &str) -> String {
    let mut characters = prompt.chars();
    let mut preview = characters
        .by_ref()
        .take(MAX_SUMMARY_PROMPT_CHARS)
        .collect::<String>();
    if characters.next().is_some() {
        preview.push('…');
    }
    preview
}

pub(crate) fn activity_time(
    captured_at_us: Option<i64>,
    first_recorded_at_us: Option<i64>,
) -> Result<ActivityTimeSummary, StoreError> {
    let (time_us, provenance) = if let Some(captured_at_us) = captured_at_us {
        (Some(captured_at_us), ActivityTimeProvenance::Captured)
    } else if let Some(first_recorded_at_us) = first_recorded_at_us {
        (
            Some(first_recorded_at_us),
            ActivityTimeProvenance::LegacyRecorded,
        )
    } else {
        (None, ActivityTimeProvenance::Unknown)
    };
    let value = time_us.map(format_rfc3339).transpose()?;
    Ok(ActivityTimeSummary { value, provenance })
}

pub(crate) fn explicit_activity_time(
    time_us: Option<i64>,
    provenance: Option<&str>,
) -> Result<ActivityTimeSummary, StoreError> {
    let provenance = match (time_us, provenance) {
        (None, None) => ActivityTimeProvenance::Unknown,
        (Some(_), Some("captured")) => ActivityTimeProvenance::Captured,
        (Some(_), Some("legacy_recorded")) => ActivityTimeProvenance::LegacyRecorded,
        _ => {
            return Err(StoreError::Invariant(
                "activity timestamp provenance is incomplete".into(),
            ));
        }
    };
    let value = time_us.map(format_rfc3339).transpose()?;
    Ok(ActivityTimeSummary { value, provenance })
}

pub(crate) fn project_summary(
    project_id: Option<i64>,
    project_name: Option<String>,
) -> Result<Option<ActivityProjectSummary>, StoreError> {
    match (project_id, project_name) {
        (Some(id), Some(name)) => Ok(Some(ActivityProjectSummary { id, name })),
        (None, None) => Ok(None),
        _ => Err(StoreError::Invariant(
            "activity project summary is incomplete".into(),
        )),
    }
}

fn format_rfc3339(time_us: i64) -> Result<String, StoreError> {
    OffsetDateTime::from_unix_timestamp_nanos(i128::from(time_us) * 1_000)
        .map_err(|error| StoreError::Invariant(format!("invalid activity timestamp: {error}")))?
        .format(&Rfc3339)
        .map_err(|error| {
            StoreError::Invariant(format!("cannot format activity timestamp: {error}"))
        })
}
