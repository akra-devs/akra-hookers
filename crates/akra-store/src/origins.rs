use std::path::Path;

use crate::{ActivityKindFilter, ActivityStore, ActivityTimeRange, OriginSummary, StoreError};

const ORIGIN_SUMMARY_SELECT: &str = "
    SELECT activity_origins.id, activity_origins.display_path, activity_origins.kind,
           activity_origins.resolution_source, activity_origins.setup_state,
           activity_origins.routing_mode, activity_origins.default_project_id, projects.name,
           COUNT(DISTINCT activity_events.id),
           COUNT(DISTINCT activity_events.provider || char(0) ||
                 activity_events.provider_session_id)
    FROM activity_origins
    LEFT JOIN projects ON projects.id = activity_origins.default_project_id
    LEFT JOIN activity_events ON activity_events.origin_id = activity_origins.id
      AND (
             activity_events.activity_kind = 'user'
          OR (?1 = 1 AND activity_events.activity_kind = 'subagent')
          OR (?2 = 1 AND activity_events.activity_kind = 'internal')
      )
      AND (
          ?3 IS NULL
          OR COALESCE(
              activity_events.captured_at_us,
              activity_events.first_recorded_at_us
          ) >= ?3
      )";

type OriginRow = (
    i64,
    String,
    String,
    String,
    String,
    String,
    Option<i64>,
    Option<String>,
    i64,
    i64,
);

impl ActivityStore {
    pub async fn origins(&self) -> Result<Vec<OriginSummary>, StoreError> {
        self.origins_filtered_in_range(ActivityKindFilter::ALL, ActivityTimeRange::ALL)
            .await
    }

    pub async fn origins_filtered_in_range(
        &self,
        activity_filter: ActivityKindFilter,
        time_range: ActivityTimeRange,
    ) -> Result<Vec<OriginSummary>, StoreError> {
        let statement = format!(
            "{ORIGIN_SUMMARY_SELECT}
             GROUP BY activity_origins.id, projects.name
             ORDER BY activity_origins.display_path, activity_origins.id"
        );
        let rows = sqlx::query_as::<_, OriginRow>(&statement)
            .bind(activity_filter.include_subagent())
            .bind(activity_filter.include_internal())
            .bind(time_range.start_at_us())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(origin_summary).collect())
    }

    pub async fn origins_for_project(
        &self,
        project_id: i64,
    ) -> Result<Vec<OriginSummary>, StoreError> {
        let exists: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM projects WHERE id = ?")
            .bind(project_id)
            .fetch_one(&self.pool)
            .await?;
        if exists == 0 {
            return Err(StoreError::ProjectNotFound(project_id));
        }
        let statement = format!(
            "{ORIGIN_SUMMARY_SELECT}
             WHERE activity_origins.default_project_id = ?4
             GROUP BY activity_origins.id, projects.name
             ORDER BY activity_origins.display_path, activity_origins.id"
        );
        let rows = sqlx::query_as::<_, OriginRow>(&statement)
            .bind(ActivityKindFilter::ALL.include_subagent())
            .bind(ActivityKindFilter::ALL.include_internal())
            .bind(ActivityTimeRange::ALL.start_at_us())
            .bind(project_id)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(origin_summary).collect())
    }

    pub async fn origin(&self, origin_id: i64) -> Result<OriginSummary, StoreError> {
        let statement = format!(
            "{ORIGIN_SUMMARY_SELECT}
             WHERE activity_origins.id = ?4
             GROUP BY activity_origins.id, projects.name"
        );
        let row = sqlx::query_as::<_, OriginRow>(&statement)
            .bind(ActivityKindFilter::ALL.include_subagent())
            .bind(ActivityKindFilter::ALL.include_internal())
            .bind(ActivityTimeRange::ALL.start_at_us())
            .bind(origin_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::OriginNotFound(origin_id))?;
        Ok(origin_summary(row))
    }
}

fn origin_summary(row: OriginRow) -> OriginSummary {
    let recommended_mode = if recommends_shared(Path::new(&row.1)) {
        "shared"
    } else {
        "dedicated"
    };
    OriginSummary {
        id: row.0,
        display_path: row.1,
        kind: row.2,
        resolution_source: row.3,
        setup_state: row.4,
        routing_mode: row.5,
        default_project_id: row.6,
        default_project_name: row.7,
        activity_count: row.8,
        conversation_count: row.9,
        recommended_mode: recommended_mode.to_owned(),
    }
}

fn recommends_shared(path: &Path) -> bool {
    if path.parent().is_none() {
        return true;
    }
    [std::env::var_os("USERPROFILE"), std::env::var_os("HOME")]
        .into_iter()
        .flatten()
        .any(|home| same_path(path, Path::new(&home)))
}

fn same_path(left: &Path, right: &Path) -> bool {
    let normalize = |path: &Path| {
        path.to_string_lossy()
            .trim_end_matches(['/', '\\'])
            .to_lowercase()
    };
    normalize(left) == normalize(right)
}
