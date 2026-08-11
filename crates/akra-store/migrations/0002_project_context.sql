PRAGMA foreign_keys = ON;

-- The migration runner records version 2 only after its conditional ALTERs,
-- legacy backfill, and foreign-key validation complete in one transaction.
CREATE TABLE IF NOT EXISTS schema_migrations (
  version INTEGER PRIMARY KEY,
  applied_at_us INTEGER NOT NULL
);

-- On an existing v1 database, the runner retains the deprecated identity and
-- display_path columns while adding the logical-project columns below.
CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  identity TEXT NOT NULL UNIQUE,
  display_path TEXT NOT NULL DEFAULT '',
  name TEXT NOT NULL,
  normalized_name TEXT NOT NULL UNIQUE,
  created_at_us INTEGER NOT NULL,
  updated_at_us INTEGER NOT NULL,
  CHECK (length(trim(name)) > 0)
);

CREATE TABLE IF NOT EXISTS activity_origins (
  id INTEGER PRIMARY KEY,
  -- A normalized Git common-directory identity or normalized directory key.
  identity TEXT NOT NULL UNIQUE,
  kind TEXT NOT NULL CHECK (kind IN ('git', 'directory', 'unresolved')),
  resolution_source TEXT NOT NULL CHECK (
    resolution_source IN ('captured', 'legacy_resolved', 'legacy_migrated')
  ),
  display_path TEXT NOT NULL DEFAULT '',
  routing_mode TEXT NOT NULL CHECK (routing_mode IN ('dedicated', 'shared')),
  default_project_id INTEGER,
  -- An unconfirmed dedicated origin's default project is its suggested name.
  setup_state TEXT NOT NULL CHECK (setup_state IN ('unconfirmed', 'confirmed')),
  created_at_us INTEGER NOT NULL,
  updated_at_us INTEGER NOT NULL,
  FOREIGN KEY(default_project_id) REFERENCES projects(id),
  CHECK (
    (routing_mode = 'dedicated' AND default_project_id IS NOT NULL)
    OR (routing_mode = 'shared' AND default_project_id IS NULL)
  )
);

CREATE TABLE IF NOT EXISTS activity_project_assignments (
  activity_event_id INTEGER PRIMARY KEY,
  project_id INTEGER NOT NULL,
  updated_at_us INTEGER NOT NULL,
  FOREIGN KEY(activity_event_id) REFERENCES activity_events(id),
  FOREIGN KEY(project_id) REFERENCES projects(id)
);

CREATE TABLE IF NOT EXISTS conversation_routes (
  provider TEXT NOT NULL,
  provider_session_id TEXT NOT NULL,
  project_id INTEGER NOT NULL,
  updated_at_us INTEGER NOT NULL,
  PRIMARY KEY(provider, provider_session_id),
  FOREIGN KEY(project_id) REFERENCES projects(id)
);

-- SQLite cannot conditionally add these columns. The versioned Rust runner
-- adds each missing column to the v1 activity_events table before backfill:
--   origin_id INTEGER REFERENCES activity_origins(id)
--   submitted_cwd TEXT
--   captured_at_us INTEGER
--   captured_at_provenance TEXT CHECK (captured_at_provenance IN ('captured'))
--   first_recorded_at_us INTEGER
--   first_recorded_at_provenance TEXT CHECK (first_recorded_at_provenance IN ('captured', 'legacy_recorded'))
--   global_sequence INTEGER
-- Existing rows intentionally retain NULL submitted_cwd and captured_at_us.

CREATE INDEX IF NOT EXISTS idx_projects_normalized_name ON projects(normalized_name);
CREATE INDEX IF NOT EXISTS idx_activity_origins_default_project
  ON activity_origins(default_project_id);
CREATE INDEX IF NOT EXISTS idx_activity_project_assignments_project
  ON activity_project_assignments(project_id);
CREATE INDEX IF NOT EXISTS idx_conversation_routes_project
  ON conversation_routes(project_id);
CREATE INDEX IF NOT EXISTS idx_activity_events_origin
  ON activity_events(origin_id);
CREATE INDEX IF NOT EXISTS idx_activity_events_conversation_sequence
  ON activity_events(provider, provider_session_id, global_sequence, id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_activity_events_global_sequence
  ON activity_events(global_sequence)
  WHERE global_sequence IS NOT NULL;
