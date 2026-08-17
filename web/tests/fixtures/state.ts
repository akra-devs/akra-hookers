import type {
  ActivityDetail,
  ActivitySummary,
  CanvasEdge,
  CanvasNode,
  OriginSummary,
  ProjectSummary,
  ProviderIntegration,
} from "../../src/api";

export type FixtureState = {
  activities: ActivitySummary[];
  details: Record<number, ActivityDetail>;
  projects: ProjectSummary[];
  origins: OriginSummary[];
  canvasNodes: CanvasNode[];
  canvasEdges: CanvasEdge[];
  provider: ProviderIntegration;
  activityOrigins: Record<number, number>;
  conversationRoutes: Record<string, number>;
  nextProjectId: number;
  nextEdgeId: number;
};

const captured = { value: "2026-08-08T12:00:00Z", provenance: "captured" } as const;
const unavailableResult = { status: "unavailable", lines: null } as const;
const unavailablePrompt = { status: "unavailable", mode: "fallback", text: null } as const;

export function createFixtureState(): FixtureState {
  const activities: ActivitySummary[] = [
    {
      id: 1,
      provider: "codex",
      activity_kind: "user",
      prompt: "프로젝트 이름을 정리해 주세요",
      project: { id: 1, name: "기존 프로젝트" },
      time: captured,
      conversation_index: 1,
      conversation_total: 2,
      result_summary_status: "unavailable",
      prompt_summary: unavailablePrompt,
    },
    {
      id: 2,
      provider: "codex",
      activity_kind: "user",
      prompt: "Inbox 항목을 분류해 주세요",
      project: null,
      time: captured,
      conversation_index: 2,
      conversation_total: 2,
      result_summary_status: "unavailable",
      prompt_summary: unavailablePrompt,
    },
  ];
  const details: Record<number, ActivityDetail> = {
    1: detail(activities[0], "turn-1"),
    2: detail(activities[1], "turn-2"),
  };
  for (const selected of [1, 2]) {
    const selectedDetail = details[selected];
    if (!selectedDetail) {
      continue;
    }
    selectedDetail.conversation = activities.map((activity) => ({
      id: activity.id,
      activity_kind: activity.activity_kind,
      prompt: activity.prompt,
      project: activity.project,
      time: activity.time,
      on_canvas: true,
      selected: activity.id === selected,
      result_summary: unavailableResult,
      prompt_summary: activity.prompt_summary,
    }));
  }
  return {
    activities,
    details,
    projects: [
      {
        id: 1,
        name: "기존 프로젝트",
        origin_count: 1,
        activity_count: 1,
        needs_setup: false,
        latest_activity_at_us: 1_786_176_000_000_000,
      },
      {
        id: 2,
        name: "미분류",
        origin_count: 1,
        activity_count: 0,
        needs_setup: true,
        latest_activity_at_us: null,
      },
    ],
    origins: [
      {
        id: 1,
        display_path: "C:\\dev\\akra-hookers",
        kind: "directory",
        resolution_source: "captured",
        setup_state: "confirmed",
        routing_mode: "dedicated",
        default_project_id: 1,
        default_project_name: "기존 프로젝트",
        activity_count: 2,
        conversation_count: 1,
        recommended_mode: "dedicated",
      },
      {
        id: 2,
        display_path: "C:\\dev\\미분류",
        kind: "unresolved",
        resolution_source: "captured",
        setup_state: "unconfirmed",
        routing_mode: "dedicated",
        default_project_id: 2,
        default_project_name: "미분류",
        activity_count: 0,
        conversation_count: 0,
        recommended_mode: "shared",
      },
    ],
    canvasNodes: [
      { id: 11, activity_event_id: 1, position_x: 80, position_y: 120 },
      { id: 12, activity_event_id: 2, position_x: 420, position_y: 220 },
    ],
    canvasEdges: [{ id: 21, source_node_id: 11, target_node_id: 12 }],
    provider: {
      provider: "codex",
      enabled: true,
      prompt_summary_mode: "off",
      collector: {
        mode: "local",
        endpoint: "http://127.0.0.1:42130",
        configured: true,
        token_configured: false,
        connected: true,
        last_delivery_at_us: null,
        pending_count: 0,
        last_error: null,
      },
      targets: [
        {
          id: "windows-native",
          label: "Codex App + CLI",
          environment: "windows",
          codex_home: "C:\\Users\\fixture\\.codex",
          hook_path: "C:\\Users\\fixture\\.codex\\hooks.json",
          enabled: true,
          available: true,
          activation: "awaiting_capture",
          clients: [
            {
              id: "app",
              label: "Codex App",
              verified: false,
              last_captured_at_us: null,
            },
            {
              id: "cli",
              label: "Codex CLI",
              verified: false,
              last_captured_at_us: null,
            },
          ],
          detail: null,
        },
        {
          id: "wsl:Ubuntu",
          label: "Codex · Ubuntu",
          environment: "wsl",
          codex_home: "/home/fixture/.codex",
          hook_path: "/home/fixture/.codex/hooks.json",
          enabled: false,
          available: true,
          activation: "disabled",
          clients: [
            {
              id: "wsl_cli",
              label: "Codex CLI · WSL",
              verified: false,
              last_captured_at_us: null,
            },
          ],
          detail: null,
        },
      ],
    },
    activityOrigins: { 1: 1, 2: 1 },
    conversationRoutes: {},
    nextProjectId: 3,
    nextEdgeId: 22,
  };
}

function detail(summary: ActivitySummary, turnId: string): ActivityDetail {
  return {
    id: summary.id,
    provider: summary.provider,
    activity_kind: summary.activity_kind,
    prompt: summary.prompt,
    project: summary.project,
    captured_at: captured,
    first_recorded_at: captured,
    on_canvas: true,
    submitted_cwd: "C:\\dev\\akra-hookers",
    origin: {
      id: 1,
      kind: "directory",
      resolution_source: "captured",
      display_path: "C:\\dev\\akra-hookers",
      activity_count: 2,
    },
    technical: {
      session_id: "fixture-session",
      turn_id: turnId,
      agent_id: null,
      agent_type: null,
    },
    result_summary: unavailableResult,
    prompt_summary: summary.prompt_summary,
    selected_turn: {
      id: summary.id,
      activity_kind: summary.activity_kind,
      prompt: summary.prompt,
      project: summary.project,
      time: summary.time,
      on_canvas: true,
      selected: true,
      result_summary: unavailableResult,
      prompt_summary: summary.prompt_summary,
    },
    conversation: [],
    conversation_index: summary.conversation_index,
    conversation_total: 2,
    conversation_has_more: false,
  };
}
