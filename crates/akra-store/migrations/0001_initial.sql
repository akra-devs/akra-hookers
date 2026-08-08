PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS projects (
  id INTEGER PRIMARY KEY,
  identity TEXT NOT NULL UNIQUE,
  display_path TEXT NOT NULL DEFAULT ''
);

CREATE TABLE IF NOT EXISTS activity_events (
  id INTEGER PRIMARY KEY,
  provider TEXT NOT NULL,
  provider_session_id TEXT NOT NULL,
  provider_turn_id TEXT NOT NULL,
  project_identity TEXT NOT NULL DEFAULT '',
  prompt TEXT NOT NULL,
  created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE(provider, provider_session_id, provider_turn_id)
);

CREATE TABLE IF NOT EXISTS canvas_nodes (
  id INTEGER PRIMARY KEY,
  activity_event_id INTEGER NOT NULL,
  position_x REAL NOT NULL DEFAULT 64,
  position_y REAL NOT NULL DEFAULT 64,
  FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
);

CREATE TABLE IF NOT EXISTS canvas_edges (
  id INTEGER PRIMARY KEY,
  source_node_id INTEGER NOT NULL,
  target_node_id INTEGER NOT NULL,
  FOREIGN KEY(source_node_id) REFERENCES canvas_nodes(id),
  FOREIGN KEY(target_node_id) REFERENCES canvas_nodes(id)
);

CREATE TABLE IF NOT EXISTS ingest_dedupes (
  provider TEXT NOT NULL,
  provider_session_id TEXT NOT NULL,
  provider_turn_id TEXT NOT NULL,
  activity_event_id INTEGER NOT NULL,
  PRIMARY KEY(provider, provider_session_id, provider_turn_id),
  FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
);

CREATE TABLE IF NOT EXISTS provider_integrations (
  provider TEXT PRIMARY KEY,
  enabled INTEGER NOT NULL,
  installation_state TEXT NOT NULL,
  last_error TEXT,
  updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS spool_receipts (
  id INTEGER PRIMARY KEY,
  spool_key TEXT NOT NULL UNIQUE,
  activity_event_id INTEGER NOT NULL,
  FOREIGN KEY(activity_event_id) REFERENCES activity_events(id)
);
