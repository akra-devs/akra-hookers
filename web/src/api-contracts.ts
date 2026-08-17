export type ActivityTimeProvenance = "captured" | "legacy_recorded" | "unknown";
export type ActivityKind = "user" | "subagent" | "internal";
export type ResultSummaryStatus = "pending" | "ready" | "unavailable" | "failed";
export type PromptSummaryStatus = "pending" | "ready" | "unavailable" | "failed";
export type PromptSummaryMode = "contextual" | "standalone" | "passthrough" | "fallback";

export type ActivityResultSummary = (
  | { status: "ready"; lines: [string, string, string] }
  | { status: Exclude<ResultSummaryStatus, "ready">; lines: null }
) & { can_regenerate: boolean };

export type ActivityPromptSummary = {
  status: PromptSummaryStatus;
  mode: PromptSummaryMode;
  text: string | null;
};

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
  activity_kind: ActivityKind;
  prompt: string;
  project: ActivityProject | null;
  time: ActivityTime;
  conversation_index: number;
  conversation_total: number;
  result_summary_status: ResultSummaryStatus;
  prompt_summary: ActivityPromptSummary;
};

export type ActivityConversationTurn = {
  id: number;
  activity_kind: ActivityKind;
  prompt: string;
  project: ActivityProject | null;
  time: ActivityTime;
  on_canvas: boolean;
  selected: boolean;
  result_summary: ActivityResultSummary;
  prompt_summary: ActivityPromptSummary;
};

export type ActivityDetail = {
  id: number;
  provider: string;
  activity_kind: ActivityKind;
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
    agent_id: string | null;
    agent_type: string | null;
  };
  result_summary: ActivityResultSummary;
  prompt_summary: ActivityPromptSummary;
  selected_turn: ActivityConversationTurn;
  conversation: ActivityConversationTurn[];
  conversation_index: number;
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

export type CodexCaptureTarget = {
  id: string;
  label: string;
  environment: string;
  codex_home: string | null;
  hook_path: string | null;
  enabled: boolean;
  available: boolean;
  activation: "disabled" | "awaiting_capture" | "verified";
  clients: CodexCaptureClient[];
  detail: string | null;
};

export type CodexCaptureClient = {
  id: "app" | "cli" | "wsl_cli" | string;
  label: string;
  verified: boolean;
  last_captured_at_us: number | null;
};

export type CollectorIntegration = {
  mode: "local" | "remote";
  endpoint: string;
  configured: boolean;
  token_configured: boolean;
  connected: boolean | null;
  last_delivery_at_us: number | null;
  pending_count: number;
  last_error: string | null;
};

export type ProviderIntegration = {
  provider: string;
  enabled: boolean;
  prompt_summary_mode: "off" | "smart";
  targets: CodexCaptureTarget[];
  collector: CollectorIntegration;
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

export type CurationLogState = "unreviewed" | "excluded" | "organized";

export type CurationLog = {
  id: number;
  project: ActivityProject;
  time: ActivityTime;
  prompt: string;
  prompt_summary: ActivityPromptSummary;
  result_summary: ActivityResultSummary;
  state: CurationLogState;
};

export type WorkLog = {
  id: number;
  time: ActivityTime;
  prompt: string;
  prompt_summary: ActivityPromptSummary;
  result_summary: ActivityResultSummary;
};

export type WorkItem = {
  id: number;
  project: ActivityProject;
  title: string;
  log_count: number;
  position_x: number;
  position_y: number;
  updated_at_us: number;
  preview_logs: WorkLog[];
};

export type WorkItemDetail = WorkItem & {
  logs: WorkLog[];
};

export type WorkEdge = {
  id: number;
  source_work_item_id: number;
  target_work_item_id: number;
};

export type CurationProposalGroup = {
  target_work_id: number | null;
  title: string;
  log_ids: number[];
  confidence: number;
  uncertain: boolean;
};

export type CurationProposal = {
  id: number;
  project_id: number;
  groups: CurationProposalGroup[];
  model: string;
  cached: boolean;
};

export type CurationApplyResult = {
  work_ids: number[];
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
