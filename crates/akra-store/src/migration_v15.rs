use sqlx::{Sqlite, Transaction};

/// Removes historical subagent captures and every relational projection that
/// depends on them. User and internal activity remain untouched.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 15")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE TEMP TABLE akra_v15_subagent_activity_ids (
             id INTEGER PRIMARY KEY
         );
         INSERT INTO akra_v15_subagent_activity_ids(id)
         SELECT id FROM activity_events WHERE activity_kind = 'subagent';

         CREATE TEMP TABLE akra_v15_subagent_canvas_node_ids (
             id INTEGER PRIMARY KEY
         );
         INSERT INTO akra_v15_subagent_canvas_node_ids(id)
         SELECT canvas_nodes.id
         FROM canvas_nodes
         JOIN akra_v15_subagent_activity_ids AS removed
           ON removed.id = canvas_nodes.activity_event_id;

         CREATE TEMP TABLE akra_v15_affected_work_item_ids (
             id INTEGER PRIMARY KEY
         );
         INSERT INTO akra_v15_affected_work_item_ids(id)
         SELECT DISTINCT work_item_logs.work_item_id
         FROM work_item_logs
         JOIN akra_v15_subagent_activity_ids AS removed
           ON removed.id = work_item_logs.activity_event_id;

         DELETE FROM activity_result_summaries
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids)
            OR EXISTS (
                SELECT 1
                FROM activity_events AS activity
                JOIN akra_v15_subagent_activity_ids AS removed
                  ON removed.id = activity.id
                WHERE activity.provider = activity_result_summaries.provider
                  AND activity.provider_session_id = activity_result_summaries.provider_session_id
                  AND activity.provider_turn_id = activity_result_summaries.provider_turn_id
            );

         DELETE FROM canvas_edges
         WHERE source_node_id IN (SELECT id FROM akra_v15_subagent_canvas_node_ids)
            OR target_node_id IN (SELECT id FROM akra_v15_subagent_canvas_node_ids);
         DELETE FROM work_item_logs
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids);
         DELETE FROM work_items
         WHERE id IN (SELECT id FROM akra_v15_affected_work_item_ids)
           AND NOT EXISTS (
               SELECT 1 FROM work_item_logs WHERE work_item_id = work_items.id
           );
         DELETE FROM activity_project_assignments
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids);
         DELETE FROM ingest_dedupes
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids);
         DELETE FROM spool_receipts
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids);
         DELETE FROM canvas_nodes
         WHERE activity_event_id IN (SELECT id FROM akra_v15_subagent_activity_ids);
         DELETE FROM activity_events
         WHERE id IN (SELECT id FROM akra_v15_subagent_activity_ids);

         UPDATE canvas_state_revision
         SET revision = revision + 1
         WHERE singleton = 1
           AND EXISTS (SELECT 1 FROM akra_v15_subagent_activity_ids);
         UPDATE work_state_revision
         SET revision = revision + 1
         WHERE singleton = 1
           AND EXISTS (SELECT 1 FROM akra_v15_affected_work_item_ids);

         DROP TABLE akra_v15_affected_work_item_ids;
         DROP TABLE akra_v15_subagent_canvas_node_ids;
         DROP TABLE akra_v15_subagent_activity_ids;",
    )
    .execute(&mut **transaction)
    .await?;

    if sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
        .fetch_optional(&mut **transaction)
        .await?
        .is_some()
    {
        return Err(sqlx::Error::Protocol("foreign key check failed".to_owned()));
    }

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (15, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use akra_core::ingress::IngressEvent;
    use akra_git::ProjectIdentity;

    use crate::{ActivityStore, RecordActivity};

    #[tokio::test]
    async fn migration_removes_only_historical_subagent_data_and_is_idempotent() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");
        let cwd = std::env::current_dir().expect("cwd");
        let event = IngressEvent::try_new(
            "codex",
            "kept-session",
            "kept-turn",
            cwd.to_string_lossy(),
            "kept user activity",
            None,
        )
        .expect("user event");
        let origin = ProjectIdentity::capture_snapshot_from_cwd(&cwd)
            .expect("origin")
            .origin;
        let user_id = store
            .record(RecordActivity::captured(event, origin, 1))
            .await
            .expect("user activity");

        let subagent_id: i64 = sqlx::query_scalar(
            "INSERT INTO activity_events (
                 provider, provider_session_id, provider_turn_id, project_identity, prompt,
                 origin_id, submitted_cwd, captured_at_us, captured_at_provenance,
                 first_recorded_at_us, first_recorded_at_provenance, global_sequence,
                 capture_target, capture_client, activity_kind, agent_id, agent_type
             )
             SELECT provider, 'historical-subagent-session', 'historical-subagent-turn',
                    project_identity, 'Subagent started: reviewer', origin_id, submitted_cwd,
                    2, 'captured', 2, 'captured',
                    (SELECT COALESCE(MAX(global_sequence), 0) + 1 FROM activity_events),
                    capture_target, capture_client, 'subagent', 'agent-7', 'reviewer'
             FROM activity_events WHERE id = ?
             RETURNING id",
        )
        .bind(user_id)
        .fetch_one(&store.pool)
        .await
        .expect("historical subagent activity");
        sqlx::query(
            "INSERT INTO ingest_dedupes (
                 provider, provider_session_id, provider_turn_id, activity_event_id
             ) VALUES ('codex', 'historical-subagent-session',
                       'historical-subagent-turn', ?)",
        )
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("subagent dedupe");
        sqlx::query("INSERT INTO canvas_nodes(activity_event_id) VALUES (?)")
            .bind(subagent_id)
            .execute(&store.pool)
            .await
            .expect("subagent node");
        let user_node: i64 =
            sqlx::query_scalar("SELECT id FROM canvas_nodes WHERE activity_event_id = ?")
                .bind(user_id)
                .fetch_one(&store.pool)
                .await
                .expect("user node");
        let subagent_node: i64 =
            sqlx::query_scalar("SELECT id FROM canvas_nodes WHERE activity_event_id = ?")
                .bind(subagent_id)
                .fetch_one(&store.pool)
                .await
                .expect("subagent node id");
        sqlx::query("INSERT INTO canvas_edges(source_node_id, target_node_id) VALUES (?, ?)")
            .bind(user_node)
            .bind(subagent_node)
            .execute(&store.pool)
            .await
            .expect("mixed edge");
        sqlx::query(
            "INSERT INTO activity_prompt_summaries (
                 activity_event_id, state, projection_kind, projection_version,
                 policy_at_capture, source_digest, summary_model, created_at_us, updated_at_us
             ) VALUES (?, 'passthrough', 'raw', 1, 'off', 'subagent-digest', 'model', 2, 2)",
        )
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("subagent prompt summary");
        sqlx::query(
            "INSERT INTO activity_result_summaries (
                 provider, provider_session_id, provider_turn_id, activity_event_id,
                 source_digest, state, generation, summary_model, captured_at_us, updated_at_us
             ) VALUES ('codex', 'historical-subagent-session',
                       'historical-subagent-turn', ?, 'result-digest', 'skipped', 1,
                       'model', 2, 2)",
        )
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("subagent result summary");
        sqlx::query(
            "INSERT INTO activity_curation_states(activity_event_id, state, updated_at_us)
             VALUES (?, 'excluded', 2)",
        )
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("subagent curation state");
        sqlx::query(
            "INSERT INTO spool_receipts(spool_key, activity_event_id)
             VALUES ('historical-subagent', ?)",
        )
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("subagent receipt");
        let project_id: i64 = sqlx::query_scalar("SELECT id FROM projects ORDER BY id LIMIT 1")
            .fetch_one(&store.pool)
            .await
            .expect("project");
        sqlx::query(
            "INSERT OR IGNORE INTO activity_project_assignments(
                 activity_event_id, project_id, updated_at_us
             ) VALUES (?, ?, 2)",
        )
        .bind(subagent_id)
        .bind(project_id)
        .execute(&store.pool)
        .await
        .expect("subagent assignment");
        let user_work_id: i64 = sqlx::query_scalar(
            "INSERT INTO work_items(project_id, title, created_at_us, updated_at_us)
             VALUES (?, 'kept work', 1, 1) RETURNING id",
        )
        .bind(project_id)
        .fetch_one(&store.pool)
        .await
        .expect("user work");
        let subagent_work_id: i64 = sqlx::query_scalar(
            "INSERT INTO work_items(project_id, title, created_at_us, updated_at_us)
             VALUES (?, 'removed work', 2, 2) RETURNING id",
        )
        .bind(project_id)
        .fetch_one(&store.pool)
        .await
        .expect("subagent work");
        sqlx::query(
            "INSERT INTO work_item_logs(work_item_id, activity_event_id, added_via, added_at_us)
             VALUES (?, ?, 'manual', 1), (?, ?, 'manual', 2)",
        )
        .bind(user_work_id)
        .bind(user_id)
        .bind(subagent_work_id)
        .bind(subagent_id)
        .execute(&store.pool)
        .await
        .expect("work logs");
        sqlx::query(
            "INSERT INTO work_edges(source_work_item_id, target_work_item_id, created_at_us)
             VALUES (?, ?, 2)",
        )
        .bind(user_work_id)
        .bind(subagent_work_id)
        .execute(&store.pool)
        .await
        .expect("work edge");

        sqlx::query("DELETE FROM schema_migrations WHERE version = 15")
            .execute(&store.pool)
            .await
            .expect("rewind v15 marker");
        let canvas_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM canvas_state_revision WHERE singleton = 1")
                .fetch_one(&store.pool)
                .await
                .expect("canvas revision");
        let work_revision: i64 =
            sqlx::query_scalar("SELECT revision FROM work_state_revision WHERE singleton = 1")
                .fetch_one(&store.pool)
                .await
                .expect("work revision");

        let mut transaction = store.pool.begin().await.expect("v15 transaction");
        super::apply(&mut transaction).await.expect("v15 migration");
        transaction.commit().await.expect("v15 commit");

        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM activity_events WHERE activity_kind = 'subagent'",
            )
            .fetch_one(&store.pool)
            .await
            .expect("subagent count"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM activity_events WHERE id = ?")
                .bind(user_id)
                .fetch_one(&store.pool)
                .await
                .expect("user count"),
            1
        );
        for (table, column) in [
            ("canvas_nodes", "activity_event_id"),
            ("ingest_dedupes", "activity_event_id"),
            ("spool_receipts", "activity_event_id"),
            ("activity_project_assignments", "activity_event_id"),
            ("activity_prompt_summaries", "activity_event_id"),
            ("activity_result_summaries", "activity_event_id"),
            ("activity_curation_states", "activity_event_id"),
            ("work_item_logs", "activity_event_id"),
        ] {
            let count: i64 =
                sqlx::query_scalar(&format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?"))
                    .bind(subagent_id)
                    .fetch_one(&store.pool)
                    .await
                    .unwrap_or_else(|error| panic!("{table} count: {error}"));
            assert_eq!(count, 0, "{table} retained subagent data");
        }
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM canvas_edges")
                .fetch_one(&store.pool)
                .await
                .expect("canvas edges"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE id = ?")
                .bind(subagent_work_id)
                .fetch_one(&store.pool)
                .await
                .expect("removed work"),
            0
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM work_items WHERE id = ?")
                .bind(user_work_id)
                .fetch_one(&store.pool)
                .await
                .expect("kept work"),
            1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM canvas_state_revision WHERE singleton = 1",
            )
            .fetch_one(&store.pool)
            .await
            .expect("new canvas revision"),
            canvas_revision + 1
        );
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM work_state_revision WHERE singleton = 1",
            )
            .fetch_one(&store.pool)
            .await
            .expect("new work revision"),
            work_revision + 1
        );
        assert!(
            sqlx::query("SELECT 1 FROM pragma_foreign_key_check LIMIT 1")
                .fetch_optional(&store.pool)
                .await
                .expect("foreign key check")
                .is_none()
        );

        let mut repeat = store.pool.begin().await.expect("repeat transaction");
        super::apply(&mut repeat)
            .await
            .expect("idempotent migration");
        repeat.commit().await.expect("repeat commit");
        assert_eq!(
            sqlx::query_scalar::<_, i64>(
                "SELECT revision FROM canvas_state_revision WHERE singleton = 1",
            )
            .fetch_one(&store.pool)
            .await
            .expect("stable canvas revision"),
            canvas_revision + 1
        );
    }
}
