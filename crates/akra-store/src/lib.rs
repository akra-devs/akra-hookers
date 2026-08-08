#![forbid(unsafe_code)]

use std::path::Path;

use serde::Serialize;
use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
};
use thiserror::Error;

#[derive(Debug)]
pub struct ActivityStore {
    pool: SqlitePool,
}

#[derive(Debug, Serialize)]
pub struct ActivitySummary {
    pub id: i64,
    pub provider: String,
    pub session_id: String,
    pub turn_id: String,
    pub prompt: String,
}

#[derive(Debug, Serialize)]
pub struct CanvasNodeSummary {
    pub id: i64,
    pub activity_event_id: i64,
    pub position_x: f64,
    pub position_y: f64,
}

#[derive(Debug, Serialize)]
pub struct CanvasEdgeSummary {
    pub id: i64,
    pub source_node_id: i64,
    pub target_node_id: i64,
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

    pub async fn migrate(&self) -> Result<(), StoreError> {
        sqlx::raw_sql(include_str!("../migrations/0001_initial.sql"))
            .execute(&self.pool)
            .await?;
        self.add_column_if_missing(
            "projects",
            "display_path",
            "ALTER TABLE projects ADD COLUMN display_path TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        self.add_column_if_missing(
            "activity_events",
            "project_identity",
            "ALTER TABLE activity_events ADD COLUMN project_identity TEXT NOT NULL DEFAULT ''",
        )
        .await?;
        sqlx::query(
            "INSERT INTO projects (identity, display_path)
             SELECT '__legacy__', 'Legacy activity'
             WHERE EXISTS (
                 SELECT 1 FROM activity_events WHERE project_identity = ''
             )
             ON CONFLICT(identity) DO NOTHING",
        )
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "UPDATE activity_events
             SET project_identity = '__legacy__'
             WHERE project_identity = ''",
        )
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn record(
        &self,
        provider: &str,
        session_id: &str,
        turn_id: &str,
        cwd: &str,
        prompt: &str,
    ) -> Result<i64, StoreError> {
        let (project_identity, project_path) = akra_git::ProjectIdentity::from_cwd(Path::new(cwd))
            .map(|identity| {
                (
                    identity.key().to_owned(),
                    identity.display_path().to_string_lossy().into_owned(),
                )
            })
            .unwrap_or_else(|_| (cwd.to_owned(), cwd.to_owned()));
        sqlx::query(
            "INSERT INTO projects (identity, display_path) VALUES (?, ?)
             ON CONFLICT(identity) DO UPDATE SET display_path = excluded.display_path",
        )
        .bind(&project_identity)
        .bind(project_path)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO activity_events (
                provider, provider_session_id, provider_turn_id, project_identity, prompt
             ) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT(provider, provider_session_id, provider_turn_id) DO NOTHING",
        )
        .bind(provider)
        .bind(session_id)
        .bind(turn_id)
        .bind(&project_identity)
        .bind(prompt)
        .execute(&self.pool)
        .await?;

        let id = sqlx::query_scalar::<_, i64>(
            "SELECT id FROM activity_events
             WHERE provider = ? AND provider_session_id = ? AND provider_turn_id = ?",
        )
        .bind(provider)
        .bind(session_id)
        .bind(turn_id)
        .fetch_one(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO canvas_nodes (activity_event_id)
             SELECT ? WHERE NOT EXISTS (
               SELECT 1 FROM canvas_nodes WHERE activity_event_id = ?
             )",
        )
        .bind(id)
        .bind(id)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn activity_count(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM activity_events")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn project_identities(&self) -> Result<Vec<String>, StoreError> {
        Ok(
            sqlx::query_scalar("SELECT identity FROM projects ORDER BY identity")
                .fetch_all(&self.pool)
                .await?,
        )
    }

    pub async fn projects(&self) -> Result<Vec<ProjectSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (String, String)>(
            "SELECT identity, display_path FROM projects ORDER BY display_path, identity",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(identity, display_path)| ProjectSummary {
            identity,
            display_path,
        })
        .collect())
    }

    pub async fn activities(&self) -> Result<Vec<ActivitySummary>, StoreError> {
        self.activities_for_project(None).await
    }

    pub async fn activities_for_project(
        &self,
        project_identity: Option<&str>,
    ) -> Result<Vec<ActivitySummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, String, String, String, String)>(
            "SELECT id, provider, provider_session_id, provider_turn_id, prompt
             FROM activity_events
             WHERE (? IS NULL OR project_identity = ?)
             ORDER BY id",
        )
        .bind(project_identity)
        .bind(project_identity)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(
            |(id, provider, session_id, turn_id, prompt)| ActivitySummary {
                id,
                provider,
                session_id,
                turn_id,
                prompt,
            },
        )
        .collect())
    }

    async fn add_column_if_missing(
        &self,
        table: &str,
        column: &str,
        statement: &str,
    ) -> Result<(), StoreError> {
        let exists = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM pragma_table_info(?) WHERE name = ?",
        )
        .bind(table)
        .bind(column)
        .fetch_one(&self.pool)
        .await?;
        if exists == 0 {
            sqlx::query(statement).execute(&self.pool).await?;
        }
        Ok(())
    }

    pub async fn canvas_nodes(&self) -> Result<Vec<CanvasNodeSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, i64, f64, f64)>(
            "SELECT id, activity_event_id, position_x, position_y FROM canvas_nodes ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(
            |(id, activity_event_id, position_x, position_y)| CanvasNodeSummary {
                id,
                activity_event_id,
                position_x,
                position_y,
            },
        )
        .collect())
    }

    pub async fn create_canvas_node(&self, activity_event_id: i64) -> Result<i64, StoreError> {
        self.create_canvas_node_at(activity_event_id, 64.0, 64.0)
            .await
    }

    pub async fn create_canvas_node_at(
        &self,
        activity_event_id: i64,
        position_x: f64,
        position_y: f64,
    ) -> Result<i64, StoreError> {
        let result = sqlx::query(
            "INSERT INTO canvas_nodes (activity_event_id, position_x, position_y) VALUES (?, ?, ?)",
        )
        .bind(activity_event_id)
        .bind(position_x)
        .bind(position_y)
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn canvas_position(
        &self,
        canvas_node_id: i64,
    ) -> Result<Option<(f64, f64)>, StoreError> {
        Ok(
            sqlx::query_as("SELECT position_x, position_y FROM canvas_nodes WHERE id = ?")
                .bind(canvas_node_id)
                .fetch_optional(&self.pool)
                .await?,
        )
    }

    pub async fn update_canvas_position(
        &self,
        canvas_node_id: i64,
        position_x: f64,
        position_y: f64,
    ) -> Result<(), StoreError> {
        sqlx::query("UPDATE canvas_nodes SET position_x = ?, position_y = ? WHERE id = ?")
            .bind(position_x)
            .bind(position_y)
            .bind(canvas_node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_canvas_node(&self, canvas_node_id: i64) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM canvas_edges WHERE source_node_id = ? OR target_node_id = ?")
            .bind(canvas_node_id)
            .bind(canvas_node_id)
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM canvas_nodes WHERE id = ?")
            .bind(canvas_node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn clear_canvas(&self) -> Result<(), StoreError> {
        sqlx::query("DELETE FROM canvas_edges")
            .execute(&self.pool)
            .await?;
        sqlx::query("DELETE FROM canvas_nodes")
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn canvas_node_exists(&self, canvas_node_id: i64) -> Result<bool, StoreError> {
        Ok(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM canvas_nodes WHERE id = ?")
                .bind(canvas_node_id)
                .fetch_one(&self.pool)
                .await?
                != 0,
        )
    }

    pub async fn create_canvas_edge(
        &self,
        source_node_id: i64,
        target_node_id: i64,
    ) -> Result<(), StoreError> {
        sqlx::query("INSERT INTO canvas_edges (source_node_id, target_node_id) VALUES (?, ?)")
            .bind(source_node_id)
            .bind(target_node_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn canvas_edge_count(&self) -> Result<i64, StoreError> {
        Ok(sqlx::query_scalar("SELECT COUNT(*) FROM canvas_edges")
            .fetch_one(&self.pool)
            .await?)
    }

    pub async fn canvas_edges(&self) -> Result<Vec<CanvasEdgeSummary>, StoreError> {
        Ok(sqlx::query_as::<_, (i64, i64, i64)>(
            "SELECT id, source_node_id, target_node_id FROM canvas_edges ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(id, source_node_id, target_node_id)| CanvasEdgeSummary {
            id,
            source_node_id,
            target_node_id,
        })
        .collect())
    }

    pub async fn set_provider_enabled(
        &self,
        provider: &str,
        enabled: bool,
    ) -> Result<(), StoreError> {
        sqlx::query(
            "INSERT INTO provider_integrations (provider, enabled, installation_state)
             VALUES (?, ?, 'configured')
             ON CONFLICT(provider) DO UPDATE SET enabled = excluded.enabled,
               updated_at = CURRENT_TIMESTAMP",
        )
        .bind(provider)
        .bind(enabled)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn provider_enabled(&self, provider: &str) -> Result<bool, StoreError> {
        Ok(sqlx::query_scalar::<_, i64>(
            "SELECT enabled FROM provider_integrations WHERE provider = ?",
        )
        .bind(provider)
        .fetch_optional(&self.pool)
        .await?
        .unwrap_or(1)
            != 0)
    }

    pub async fn provider(&self, provider: &str) -> Result<ProviderIntegration, StoreError> {
        let enabled = self.provider_enabled(provider).await?;
        Ok(ProviderIntegration {
            provider: provider.to_owned(),
            enabled,
        })
    }
}

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("SQLite operation failed: {0}")]
    Sqlite(#[from] sqlx::Error),
}

#[derive(Debug, serde::Serialize)]
pub struct ProviderIntegration {
    pub provider: String,
    pub enabled: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ProjectSummary {
    pub identity: String,
    pub display_path: String,
}
