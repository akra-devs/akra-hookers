export type ActivityTimeProvenance = "captured" | "legacy_recorded" | "unknown";

export type ActivityTime = {
  value: string | null;
  provenance: ActivityTimeProvenance;
};

export type ActivityProject = {
  id: number;
  name: string;
};

export type ActivitySummary = {
  id: number;
  provider: string;
  prompt: string;
  project: ActivityProject | null;
  time: ActivityTime;
  conversation_index: number;
  conversation_total: number;
};

export type ActivityConversationTurn = {
  id: number;
  prompt: string;
  project: ActivityProject | null;
  time: ActivityTime;
  on_canvas: boolean;
  selected: boolean;
};

export type ActivityDetail = {
  id: number;
  provider: string;
  prompt: string;
  project: ActivityProject | null;
  captured_at: ActivityTime;
  first_recorded_at: ActivityTime;
  on_canvas: boolean;
  submitted_cwd: string | null;
  origin: {
    id: number;
    kind: "git" | "directory" | "unresolved";
    resolution_source: "captured" | "legacy_resolved" | "legacy_migrated";
    display_path: string;
    activity_count: number;
  };
  technical: {
    session_id: string;
    turn_id: string;
  };
  selected_turn: ActivityConversationTurn;
  conversation: ActivityConversationTurn[];
  conversation_total: number;
  conversation_has_more: boolean;
};

export type ProjectSummary = {
  id: number;
  name: string;
  origin_count: number;
  activity_count: number;
  needs_setup: boolean;
  latest_activity_at_us: number | null;
};

export type OriginSummary = {
  id: number;
  display_path: string;
  kind: "git" | "directory" | "unresolved";
  resolution_source: "captured" | "legacy_resolved" | "legacy_migrated";
  setup_state: "confirmed" | "unconfirmed";
  routing_mode: "dedicated" | "shared";
  default_project_id: number | null;
  default_project_name: string | null;
  activity_count: number;
  conversation_count: number;
  recommended_mode: "dedicated" | "shared";
};

export type ProviderIntegration = {
  provider: string;
  enabled: boolean;
};

export type CanvasNode = {
  id: number;
  activity_event_id: number;
  position_x: number;
  position_y: number;
};

export type CanvasEdge = {
  id: number;
  source_node_id: number;
  target_node_id: number;
};

export type ActivityScope =
  | { scope: "all" }
  | { scope: "inbox" }
  | { scope: "project"; projectId: number };

export type ProjectDestination =
  | { project_id: number }
  | { new_project_name: string };

export type OriginRoutingRequest =
  | { mode: "shared"; confirm: boolean }
  | { mode: "dedicated"; destination: ProjectDestination; confirm: boolean };

export type FutureRoute = "unchanged" | "set" | "clear";

export type ActivityAssignmentRequest = {
  activity_ids: number[];
  destination: ProjectDestination | null;
  future_route?: FutureRoute;
};

export type ActivityAssignmentResult = {
  activity_ids: number[];
  project_id: number | null;
  future_route: FutureRoute;
};

export type ApiErrorBody = {
  code: string;
  message: string;
};
