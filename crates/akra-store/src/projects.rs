use sqlx::SqliteConnection;

use crate::{
    ActivityKindFilter, ActivityStore, ActivityTimeRange, ProjectName, ProjectSummary, StoreError,
};

const PROJECT_SUMMARY_SELECT: &str = "
    SELECT projects.id, projects.name,
           (SELECT COUNT(*) FROM activity_origins
            WHERE default_project_id = projects.id),
           (SELECT COUNT(*) FROM activity_events
            WHERE activity_events.deleted_at_us IS NULL
              AND (
                EXISTS (
                    SELECT 1 FROM activity_project_assignments
                    WHERE activity_event_id = activity_events.id
                      AND project_id = projects.id
                ) OR EXISTS (
                    SELECT 1 FROM activity_origins
                    WHERE id = activity_events.origin_id
                      AND routing_mode = 'dedicated'
                      AND default_project_id = projects.id
                ) OR (
                    origin_id IS NULL
                    AND
                    project_identity = projects.identity
                    AND NOT EXISTS (
                        SELECT 1 FROM activity_project_assignments
                        WHERE activity_event_id = activity_events.id
                    )
                )
            ) AND (
                   activity_events.activity_kind = 'user'
                OR (?1 = 1 AND activity_events.activity_kind = 'subagent')
                OR (?2 = 1 AND activity_events.activity_kind = 'internal')
            ) AND (
                ?3 IS NULL
                OR COALESCE(
                    activity_events.captured_at_us,
                    activity_events.first_recorded_at_us
                ) >= ?3
            )),
           CASE WHEN EXISTS (
               SELECT 1 FROM activity_origins
               WHERE default_project_id = projects.id
                 AND setup_state = 'unconfirmed'
           ) THEN 1 ELSE 0 END,
           (SELECT MAX(COALESCE(captured_at_us, first_recorded_at_us))
            FROM activity_events
            WHERE activity_events.deleted_at_us IS NULL
              AND (
                EXISTS (
                    SELECT 1 FROM activity_project_assignments
                    WHERE activity_event_id = activity_events.id
                      AND project_id = projects.id
                ) OR EXISTS (
                    SELECT 1 FROM activity_origins
                    WHERE id = activity_events.origin_id
                      AND routing_mode = 'dedicated'
                      AND default_project_id = projects.id
                ) OR (
                    origin_id IS NULL
                    AND
                    project_identity = projects.identity
                    AND NOT EXISTS (
                        SELECT 1 FROM activity_project_assignments
                        WHERE activity_event_id = activity_events.id
                    )
                )
            ) AND (
                   activity_events.activity_kind = 'user'
                OR (?1 = 1 AND activity_events.activity_kind = 'subagent')
                OR (?2 = 1 AND activity_events.activity_kind = 'internal')
            ) AND (
                ?3 IS NULL
                OR COALESCE(
                    activity_events.captured_at_us,
                    activity_events.first_recorded_at_us
                ) >= ?3
            ))
    FROM projects";

impl ActivityStore {
    pub async fn project_identities(&self) -> Result<Vec<String>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT identity FROM projects ORDER BY identity")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn projects(&self) -> Result<Vec<ProjectSummary>, StoreError> {
        self.projects_filtered_in_range(ActivityKindFilter::ALL, ActivityTimeRange::ALL)
            .await
    }

    pub async fn projects_filtered(
        &self,
        activity_filter: ActivityKindFilter,
    ) -> Result<Vec<ProjectSummary>, StoreError> {
        self.projects_filtered_in_range(activity_filter, ActivityTimeRange::ALL)
            .await
    }

    pub async fn projects_filtered_in_range(
        &self,
        activity_filter: ActivityKindFilter,
        time_range: ActivityTimeRange,
    ) -> Result<Vec<ProjectSummary>, StoreError> {
        let statement =
            format!("{PROJECT_SUMMARY_SELECT} ORDER BY projects.normalized_name, projects.id");
        let rows = sqlx::query_as::<_, ProjectRow>(&statement)
            .bind(activity_filter.include_subagent())
            .bind(activity_filter.include_internal())
            .bind(time_range.start_at_us())
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(project_summary).collect())
    }

    pub async fn create_project(&self, raw_name: &str) -> Result<ProjectSummary, StoreError> {
        let mut transaction = self.pool.begin().await?;
        let id = create_project_in(&mut transaction, raw_name).await?;
        transaction.commit().await?;
        self.project(id).await
    }

    pub async fn rename_project(
        &self,
        project_id: i64,
        raw_name: &str,
    ) -> Result<ProjectSummary, StoreError> {
        let mut transaction = self.pool.begin().await?;
        rename_project_in(&mut transaction, project_id, raw_name).await?;
        transaction.commit().await?;
        self.project(project_id).await
    }

    pub async fn merge_projects(
        &self,
        source_project_id: i64,
        target_project_id: i64,
    ) -> Result<ProjectSummary, StoreError> {
        if source_project_id == target_project_id {
            return Err(StoreError::SameProjectMerge);
        }
        let mut transaction = self.pool.begin().await?;
        ensure_project_exists(&mut transaction, source_project_id).await?;
        ensure_project_exists(&mut transaction, target_project_id).await?;
        sqlx::query(
            "UPDATE activity_origins SET
                 default_project_id = ?,
                 updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             WHERE default_project_id = ?",
        )
        .bind(target_project_id)
        .bind(source_project_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE activity_project_assignments SET
                 project_id = ?,
                 updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             WHERE project_id = ?",
        )
        .bind(target_project_id)
        .bind(source_project_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query(
            "UPDATE conversation_routes SET
                 project_id = ?,
                 updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
             WHERE project_id = ?",
        )
        .bind(target_project_id)
        .bind(source_project_id)
        .execute(&mut *transaction)
        .await?;
        sqlx::query("DELETE FROM projects WHERE id = ?")
            .bind(source_project_id)
            .execute(&mut *transaction)
            .await?;
        transaction.commit().await?;
        self.project(target_project_id).await
    }

    async fn project(&self, project_id: i64) -> Result<ProjectSummary, StoreError> {
        let statement = format!("{PROJECT_SUMMARY_SELECT} WHERE projects.id = ?4");
        let row = sqlx::query_as::<_, ProjectRow>(&statement)
            .bind(ActivityKindFilter::ALL.include_subagent())
            .bind(ActivityKindFilter::ALL.include_internal())
            .bind(ActivityTimeRange::ALL.start_at_us())
            .bind(project_id)
            .fetch_optional(&self.pool)
            .await?
            .ok_or(StoreError::ProjectNotFound(project_id))?;
        Ok(project_summary(row))
    }
}

type ProjectRow = (i64, String, i64, i64, i64, Option<i64>);

fn project_summary(row: ProjectRow) -> ProjectSummary {
    ProjectSummary {
        id: row.0,
        name: row.1,
        origin_count: row.2,
        activity_count: row.3,
        needs_setup: row.4 != 0,
        latest_activity_at_us: row.5,
    }
}

pub(crate) async fn create_project_in(
    connection: &mut SqliteConnection,
    raw_name: &str,
) -> Result<i64, StoreError> {
    let name = ProjectName::parse(raw_name)?;
    ensure_name_available(connection, name.normalized(), -1).await?;
    let id = allocate_project_id(connection).await?;
    sqlx::query(
        "INSERT INTO projects (
             id, identity, display_path, name, normalized_name, created_at_us, updated_at_us
         ) VALUES (
             ?, ?, '', ?, ?,
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER),
             CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         )",
    )
    .bind(id)
    .bind(format!("manual:{id}"))
    .bind(name.display())
    .bind(name.normalized())
    .execute(&mut *connection)
    .await?;
    Ok(id)
}

pub(crate) async fn allocate_project_id(
    connection: &mut SqliteConnection,
) -> Result<i64, StoreError> {
    Ok(sqlx::query_scalar(
        "UPDATE project_id_allocator
         SET next_id = MAX(
             next_id,
             (SELECT COALESCE(MAX(id), 0) + 1 FROM projects)
         ) + 1
         WHERE singleton = 1
         RETURNING next_id - 1",
    )
    .fetch_one(&mut *connection)
    .await?)
}

pub(crate) async fn rename_project_in(
    connection: &mut SqliteConnection,
    project_id: i64,
    raw_name: &str,
) -> Result<(), StoreError> {
    let name = ProjectName::parse(raw_name)?;
    ensure_project_exists(connection, project_id).await?;
    ensure_name_available(connection, name.normalized(), project_id).await?;
    sqlx::query(
        "UPDATE projects SET
             name = ?, normalized_name = ?,
             updated_at_us = CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER)
         WHERE id = ?",
    )
    .bind(name.display())
    .bind(name.normalized())
    .bind(project_id)
    .execute(&mut *connection)
    .await?;
    Ok(())
}

pub(crate) async fn ensure_project_exists(
    connection: &mut SqliteConnection,
    project_id: i64,
) -> Result<(), StoreError> {
    let exists = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM projects WHERE id = ?")
        .bind(project_id)
        .fetch_one(&mut *connection)
        .await?;
    if exists == 0 {
        return Err(StoreError::ProjectNotFound(project_id));
    }
    Ok(())
}

async fn ensure_name_available(
    connection: &mut SqliteConnection,
    normalized_name: &str,
    excluding_id: i64,
) -> Result<(), StoreError> {
    let exists = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM projects WHERE normalized_name = ? AND id <> ?",
    )
    .bind(normalized_name)
    .bind(excluding_id)
    .fetch_one(&mut *connection)
    .await?;
    if exists != 0 {
        return Err(StoreError::ProjectNameConflict);
    }
    Ok(())
}
