use sqlx::{Sqlite, Transaction};

/// Adds user-confirmed work memory on top of the retained activity evidence.
///
/// Work membership is intentionally one-to-one in v1, and AI proposals remain
/// inert until an explicit apply transaction succeeds.
pub(crate) async fn apply(transaction: &mut Transaction<'_, Sqlite>) -> Result<(), sqlx::Error> {
    if sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM schema_migrations WHERE version = 12")
        .fetch_one(&mut **transaction)
        .await?
        != 0
    {
        return Ok(());
    }

    sqlx::raw_sql(
        "CREATE INDEX idx_activity_events_visible_sequence
             ON activity_events(deleted_at_us, global_sequence, id);

         CREATE TABLE work_items (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE RESTRICT,
             title TEXT NOT NULL CHECK(length(trim(title)) BETWEEN 1 AND 80),
             position_x REAL NOT NULL DEFAULT 96,
             position_y REAL NOT NULL DEFAULT 96,
             created_at_us INTEGER NOT NULL,
             updated_at_us INTEGER NOT NULL,
             deleted_at_us INTEGER
         );

         CREATE INDEX idx_work_items_project_active
             ON work_items(project_id, updated_at_us DESC, id)
             WHERE deleted_at_us IS NULL;

         CREATE TABLE work_item_logs (
             work_item_id INTEGER NOT NULL
                 REFERENCES work_items(id) ON DELETE CASCADE,
             activity_event_id INTEGER NOT NULL UNIQUE
                 REFERENCES activity_events(id) ON DELETE RESTRICT,
             added_via TEXT NOT NULL
                 CHECK(added_via IN ('manual', 'ai_confirmed')),
             added_at_us INTEGER NOT NULL,
             PRIMARY KEY(work_item_id, activity_event_id)
         );

         CREATE INDEX idx_work_item_logs_work
             ON work_item_logs(work_item_id, added_at_us, activity_event_id);

         CREATE TABLE activity_curation_states (
             activity_event_id INTEGER PRIMARY KEY
                 REFERENCES activity_events(id) ON DELETE CASCADE,
             state TEXT NOT NULL CHECK(state = 'excluded'),
             updated_at_us INTEGER NOT NULL
         );

         CREATE TABLE work_edges (
             id INTEGER PRIMARY KEY,
             source_work_item_id INTEGER NOT NULL
                 REFERENCES work_items(id) ON DELETE CASCADE,
             target_work_item_id INTEGER NOT NULL
                 REFERENCES work_items(id) ON DELETE CASCADE,
             created_at_us INTEGER NOT NULL,
             CHECK(source_work_item_id <> target_work_item_id),
             UNIQUE(source_work_item_id, target_work_item_id)
         );

         CREATE TABLE work_state_revision (
             singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
             revision INTEGER NOT NULL CHECK(revision >= 0)
         );
         INSERT INTO work_state_revision(singleton, revision) VALUES (1, 0);

         CREATE TABLE work_curation_proposals (
             id INTEGER PRIMARY KEY,
             project_id INTEGER NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
             fingerprint TEXT NOT NULL UNIQUE,
             selected_activity_ids_json TEXT NOT NULL,
             proposal_json TEXT NOT NULL,
             state TEXT NOT NULL CHECK(state IN ('ready', 'applied', 'discarded')),
             summary_model TEXT NOT NULL,
             created_at_us INTEGER NOT NULL,
             updated_at_us INTEGER NOT NULL
         );

         CREATE INDEX idx_work_curation_proposals_project
             ON work_curation_proposals(project_id, created_at_us DESC, id);",
    )
    .execute(&mut **transaction)
    .await?;

    sqlx::query(
        "INSERT INTO schema_migrations (version, applied_at_us)
         VALUES (12, CAST((julianday('now') - 2440587.5) * 86400000000 AS INTEGER))",
    )
    .execute(&mut **transaction)
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::ActivityStore;

    #[tokio::test]
    async fn migration_is_idempotent_and_starts_with_an_empty_work_memory() {
        let store = ActivityStore::in_memory().await.expect("store");
        store.migrate().await.expect("migrations");

        let work_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM work_items")
            .fetch_one(&store.pool)
            .await
            .expect("work count");
        let revision: i64 =
            sqlx::query_scalar("SELECT revision FROM work_state_revision WHERE singleton = 1")
                .fetch_one(&store.pool)
                .await
                .expect("revision");
        assert_eq!(work_count, 0);
        assert_eq!(revision, 0);

        let mut transaction = store.pool.begin().await.expect("transaction");
        super::apply(&mut transaction)
            .await
            .expect("idempotent migration");
        transaction.commit().await.expect("commit");
    }
}
