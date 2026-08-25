import type {
  ActivityConversationTurn,
  ActivityDetail,
  ActivityPromptSummary,
  ActivityResultSummary,
  ActivitySummary,
  CurationLog,
  WorkItemDetail,
  WorkLog,
} from "../src/api";
import { createFixtureState, type FixtureState } from "../tests/fixtures/state";

type Seed = {
  id: number;
  projectId: number | null;
  originId: number;
  sessionId: string;
  activityKind: "user" | "subagent" | "internal";
  rawPrompt: string;
  promptSummary: string;
  result: ActivityResultSummary;
  capturedAtMs: number;
  agentId?: string;
  agentType?: string;
};

const HOUR_MS = 60 * 60 * 1_000;

function kstDayStartMs(nowMs: number): number {
  const values = Object.fromEntries(
    new Intl.DateTimeFormat("en-CA", {
      timeZone: "Asia/Seoul",
      year: "numeric",
      month: "2-digit",
      day: "2-digit",
    }).formatToParts(new Date(nowMs)).map(({ type, value }) => [type, value]),
  );
  return Date.UTC(
    Number(values.year),
    Number(values.month) - 1,
    Number(values.day),
  ) - 9 * HOUR_MS;
}

function ready(lines: [string, string, string]): ActivityResultSummary {
  return { status: "ready", lines, can_regenerate: false };
}

function prompt(text: string): ActivityPromptSummary {
  return { status: "ready", mode: "contextual", text };
}

function activityTime(capturedAtMs: number) {
  return { value: new Date(capturedAtMs).toISOString(), provenance: "captured" as const };
}

function toWorkLog(log: CurationLog): WorkLog {
  return {
    id: log.id,
    time: log.time,
    prompt: log.prompt,
    prompt_summary: log.prompt_summary,
    result_summary: log.result_summary,
  };
}

export function createShowcaseState(nowMs = Date.now()): FixtureState {
  const state = createFixtureState();
  const dayStartMs = kstDayStartMs(nowMs);
  const todayElapsedMs = Math.max(60_000, nowMs - dayStartMs);
  const todayAt = (ratio: number) => dayStartMs + todayElapsedMs * ratio;
  const previousDayWithin24Hours = dayStartMs - Math.max(
    1_000,
    Math.min(30 * 60_000, (24 * HOUR_MS - todayElapsedMs) / 2),
  );

  const projects = [
    {
      id: 1,
      name: "Akra Hookers",
      origin_count: 1,
      activity_count: 7,
      needs_setup: false,
      latest_activity_at_us: Math.round(todayAt(0.92) * 1_000),
    },
    {
      id: 2,
      name: "Waxball",
      origin_count: 1,
      activity_count: 2,
      needs_setup: false,
      latest_activity_at_us: Math.round(previousDayWithin24Hours * 1_000),
    },
    {
      id: 3,
      name: "출시 아이디어",
      origin_count: 1,
      activity_count: 0,
      needs_setup: false,
      latest_activity_at_us: null,
    },
  ];
  const projectById = new Map(projects.map((project) => [project.id, project]));

  const seeds: Seed[] = [
    {
      id: 1,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-capture-hardening",
      activityKind: "user",
      rawPrompt: "Windows와 WSL에서 같은 CODEX_HOME을 쓰더라도 hook 명령과 신뢰 상태가 안전하게 유지되도록 정리해 주세요.",
      promptSummary: "Windows·WSL 공용 hook 경로 안정화",
      result: ready([
        "공용 manifest에 Windows와 WSL 명령을 분리해 보존했습니다.",
        "설치별 선택 상태와 hook 신뢰 해시를 함께 검증했습니다.",
        "재시작 뒤에도 기존 설정이 안전하게 복구됩니다.",
      ]),
      capturedAtMs: nowMs - 10 * 24 * HOUR_MS,
    },
    {
      id: 2,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-capture-hardening",
      activityKind: "user",
      rawPrompt: "좋습니다. 결과 요약 worker가 원문을 오래 들고 있지 않도록 보존 기한과 역순 Stop 경합까지 계속 진행해 주세요.",
      promptSummary: "결과 요약 보존·경합 방어 마무리",
      result: ready([
        "24시간 보존 상한과 stale 결과 방어를 적용했습니다.",
        "Stop 순서가 뒤집혀도 최신 응답이 유지됩니다.",
        "워크스페이스 회귀 테스트를 모두 통과했습니다.",
      ]),
      capturedAtMs: nowMs - 26 * HOUR_MS,
    },
    {
      id: 3,
      projectId: 2,
      originId: 2,
      sessionId: "showcase-waxball",
      activityKind: "user",
      rawPrompt: "Waxball 충돌 강도에 따라 유리 파편과 타격음이 자연스럽게 달라지는 촉각 피드백을 설계해 주세요.",
      promptSummary: "충돌 강도별 촉각 피드백 설계",
      result: ready([
        "충돌 에너지를 세 단계 감각 이벤트로 정규화했습니다.",
        "파편·오디오·햅틱이 같은 타임라인을 따릅니다.",
        "저사양 기기에서도 결정적인 결과를 재현합니다.",
      ]),
      capturedAtMs: nowMs - 25 * HOUR_MS,
    },
    {
      id: 4,
      projectId: 2,
      originId: 2,
      sessionId: "showcase-waxball",
      activityKind: "user",
      rawPrompt: "방금 설계를 기준으로 press-and-hold 가속과 파괴 직전 긴장감을 구현하고 실제 기기에서 검증해 주세요.",
      promptSummary: "홀드 가속과 파괴 직전 피드백 검증",
      result: ready([
        "홀드 시간에 따른 가속 곡선과 상한을 적용했습니다.",
        "파괴 직전에는 시각·햅틱 신호가 단계적으로 쌓입니다.",
        "실기기 프레임과 감각 이벤트 순서를 검증했습니다.",
      ]),
      capturedAtMs: previousDayWithin24Hours,
    },
    {
      id: 5,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "user",
      rawPrompt: "portable 빌드와 설치 빌드가 모두 %LOCALAPPDATA%\\akra-hookers를 공식 데이터 경로로 사용하게 만들고 Linux/macOS 분기도 준비해 주세요.",
      promptSummary: "플랫폼 공통 공식 데이터 경로 적용",
      result: ready([
        "Windows 빌드의 데이터 경로를 LocalAppData로 통일했습니다.",
        "Linux와 macOS의 표준 사용자 데이터 경로도 분기했습니다.",
        "기존 데이터 탐색과 마이그레이션 검증을 추가했습니다.",
      ]),
      capturedAtMs: todayAt(0.38),
    },
    {
      id: 6,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "user",
      rawPrompt: "기간 필터에 오늘과 24시간을 정확히 구분하고 프로젝트 숫자와 결과 없는 프로젝트 숨기기까지 같은 조건으로 동작시켜 주세요.",
      promptSummary: "오늘·24시간 필터와 프로젝트 수 정합성",
      result: ready([
        "오늘은 로컬 날짜 경계, 24시간은 현재 시각 기준으로 계산합니다.",
        "프로젝트·Inbox·전체 활동 수가 같은 필터를 공유합니다.",
        "결과 없는 프로젝트를 즉시 숨길 수 있습니다.",
      ]),
      capturedAtMs: todayAt(0.55),
    },
    {
      id: 7,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "user",
      rawPrompt: "release page에서 실제 앱 사용법이 느껴지도록 왼쪽 탐색, 중앙 캔버스, 로그 정리 과정을 실제 캡처로 크게 보여 주세요.",
      promptSummary: "실제 화면 중심 배포 페이지 구성",
      result: { status: "failed", lines: null, can_regenerate: true },
      capturedAtMs: todayAt(0.7),
    },
    {
      id: 8,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "subagent",
      rawPrompt: "You are an expert reviewer. Audit the release experience for accessibility, responsive layout, and evidence fidelity.",
      promptSummary: "배포 경험 접근성·반응형 검토",
      result: ready([
        "다운로드 CTA와 실제 앱 증거의 우선순위를 점검했습니다.",
        "모바일 오버플로와 키보드 탐색 계약을 확인했습니다.",
        "출하를 막는 추가 UI 회귀는 발견되지 않았습니다.",
      ]),
      capturedAtMs: todayAt(0.79),
      agentId: "agent-showcase-review",
      agentType: "reviewer",
    },
    {
      id: 9,
      projectId: 1,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "internal",
      rawPrompt: "You are an expert at upholding safety and compliance standards for Codex ambient suggestions.",
      promptSummary: "Codex ambient safety check",
      result: { status: "unavailable", lines: null, can_regenerate: false },
      capturedAtMs: todayAt(0.85),
    },
    {
      id: 10,
      projectId: null,
      originId: 1,
      sessionId: "showcase-release",
      activityKind: "user",
      rawPrompt: "이어서 진행해 주세요.",
      promptSummary: "실제 앱 시연 영상 제작을 계속 진행",
      result: { status: "pending", lines: null, can_regenerate: false },
      capturedAtMs: todayAt(0.92),
    },
  ];

  const sessionSeeds = new Map<string, Seed[]>();
  for (const seed of seeds) {
    sessionSeeds.set(seed.sessionId, [...(sessionSeeds.get(seed.sessionId) ?? []), seed]);
  }

  const summaries: ActivitySummary[] = seeds.map((seed) => {
    const conversation = sessionSeeds.get(seed.sessionId) ?? [];
    const project = seed.projectId === null
      ? null
      : { id: seed.projectId, name: projectById.get(seed.projectId)?.name ?? "Unknown" };
    return {
      id: seed.id,
      provider: "codex",
      activity_kind: seed.activityKind,
      prompt: seed.rawPrompt,
      project,
      time: activityTime(seed.capturedAtMs),
      previous_conversation_activity_id: conversation
        .slice(0, conversation.findIndex(({ id }) => id === seed.id))
        .at(-1)?.id ?? null,
      conversation_index: conversation.findIndex(({ id }) => id === seed.id) + 1,
      conversation_total: conversation.length,
      result_summary_status: seed.result.status,
      prompt_summary: prompt(seed.promptSummary),
    };
  });
  const summaryById = new Map(summaries.map((summary) => [summary.id, summary]));

  const details: Record<number, ActivityDetail> = {};
  for (const seed of seeds) {
    const summary = summaryById.get(seed.id)!;
    const conversationSeeds = sessionSeeds.get(seed.sessionId) ?? [];
    const turns: ActivityConversationTurn[] = conversationSeeds.map((candidate) => {
      const candidateSummary = summaryById.get(candidate.id)!;
      return {
        id: candidate.id,
        activity_kind: candidate.activityKind,
        prompt: candidate.rawPrompt,
        project: candidateSummary.project,
        time: candidateSummary.time,
        on_canvas: true,
        selected: candidate.id === seed.id,
        result_summary: structuredClone(candidate.result),
        prompt_summary: prompt(candidate.promptSummary),
      };
    });
    const selectedTurn = turns.find(({ id }) => id === seed.id)!;
    const origin = seed.originId === 2
      ? { id: 2, path: "C:\\dev\\akra-waxball-flutter", count: 2 }
      : { id: 1, path: "C:\\dev\\akra-hookers", count: 8 };
    details[seed.id] = {
      id: seed.id,
      provider: "codex",
      activity_kind: seed.activityKind,
      prompt: seed.rawPrompt,
      project: summary.project,
      captured_at: summary.time,
      first_recorded_at: summary.time,
      on_canvas: true,
      submitted_cwd: origin.path,
      origin: {
        id: origin.id,
        kind: "git",
        resolution_source: "captured",
        display_path: origin.path,
        activity_count: origin.count,
      },
      technical: {
        session_id: seed.sessionId,
        turn_id: `turn-${seed.id}`,
        agent_id: seed.agentId ?? null,
        agent_type: seed.agentType ?? null,
      },
      result_summary: structuredClone(seed.result),
      prompt_summary: prompt(seed.promptSummary),
      selected_turn: structuredClone(selectedTurn),
      conversation: turns,
      conversation_index: summary.conversation_index,
      conversation_total: summary.conversation_total,
      conversation_has_more: false,
    };
  }

  const curationLogs: CurationLog[] = seeds
    .filter((seed) => seed.activityKind === "user" && seed.projectId !== null)
    .map((seed) => {
      const summary = summaryById.get(seed.id)!;
      return {
        id: seed.id,
        project: summary.project!,
        time: summary.time,
        prompt: seed.rawPrompt,
        prompt_summary: prompt(seed.promptSummary),
        result_summary: structuredClone(seed.result),
        state: seed.id <= 4 ? "organized" : "unreviewed",
      };
    });
  const logsById = new Map(curationLogs.map((log) => [log.id, log]));
  const work = (
    id: number,
    projectId: number,
    title: string,
    logIds: number[],
    positionX: number,
    positionY: number,
  ): WorkItemDetail => {
    const logs = logIds.map((id) => toWorkLog(logsById.get(id)!));
    const project = projectById.get(projectId)!;
    return {
      id,
      project: { id: project.id, name: project.name },
      title,
      log_count: logs.length,
      position_x: positionX,
      position_y: positionY,
      updated_at_us: Math.round(nowMs * 1_000) - id,
      preview_logs: logs.slice(0, 2),
      logs,
    };
  };

  state.activities = summaries;
  state.details = details;
  state.projects = projects;
  state.origins = [
    {
      id: 1,
      display_path: "C:\\dev\\akra-hookers",
      kind: "git",
      resolution_source: "captured",
      setup_state: "confirmed",
      routing_mode: "dedicated",
      default_project_id: 1,
      default_project_name: "Akra Hookers",
      activity_count: 8,
      conversation_count: 3,
      recommended_mode: "dedicated",
    },
    {
      id: 2,
      display_path: "C:\\dev\\akra-waxball-flutter",
      kind: "git",
      resolution_source: "captured",
      setup_state: "confirmed",
      routing_mode: "dedicated",
      default_project_id: 2,
      default_project_name: "Waxball",
      activity_count: 2,
      conversation_count: 1,
      recommended_mode: "dedicated",
    },
    {
      id: 3,
      display_path: "C:\\dev\\release-ideas",
      kind: "directory",
      resolution_source: "captured",
      setup_state: "confirmed",
      routing_mode: "dedicated",
      default_project_id: 3,
      default_project_name: "출시 아이디어",
      activity_count: 0,
      conversation_count: 0,
      recommended_mode: "dedicated",
    },
  ];
  state.canvasNodes = [
    { id: 101, activity_event_id: 1, position_x: -720, position_y: 0 },
    { id: 102, activity_event_id: 2, position_x: -360, position_y: 0 },
    { id: 103, activity_event_id: 3, position_x: -720, position_y: 330 },
    { id: 104, activity_event_id: 4, position_x: -360, position_y: 330 },
    { id: 105, activity_event_id: 5, position_x: 0, position_y: 0 },
    { id: 106, activity_event_id: 6, position_x: 360, position_y: 0 },
    { id: 107, activity_event_id: 7, position_x: 720, position_y: 0 },
    { id: 108, activity_event_id: 8, position_x: 360, position_y: 330 },
    { id: 109, activity_event_id: 9, position_x: 720, position_y: 330 },
    { id: 110, activity_event_id: 10, position_x: 0, position_y: 330 },
  ];
  state.canvasEdges = [
    { id: 201, source_node_id: 101, target_node_id: 102 },
    { id: 202, source_node_id: 103, target_node_id: 104 },
    { id: 203, source_node_id: 105, target_node_id: 106 },
    { id: 204, source_node_id: 106, target_node_id: 107 },
    { id: 205, source_node_id: 107, target_node_id: 108 },
    { id: 206, source_node_id: 107, target_node_id: 110 },
  ];
  state.provider = {
    provider: "codex",
    enabled: true,
    prompt_summary_mode: "smart",
    collector: {
      mode: "local",
      endpoint: "http://127.0.0.1:42130",
      configured: true,
      token_configured: false,
      connected: true,
      last_delivery_at_us: Math.round(todayAt(0.92) * 1_000),
      pending_count: 0,
      last_error: null,
    },
    targets: [
      {
        id: "windows-native",
        label: "Codex App + CLI",
        environment: "windows",
        codex_home: "C:\\Users\\showcase\\.codex",
        hook_path: "C:\\Users\\showcase\\.codex\\hooks.json",
        enabled: true,
        available: true,
        activation: "verified",
        clients: [
          {
            id: "app",
            label: "Codex App",
            verified: true,
            last_captured_at_us: Math.round(todayAt(0.92) * 1_000),
          },
          {
            id: "cli",
            label: "Codex CLI",
            verified: true,
            last_captured_at_us: Math.round(todayAt(0.7) * 1_000),
          },
        ],
        detail: null,
      },
      {
        id: "wsl:Ubuntu",
        label: "Codex · Ubuntu",
        environment: "wsl",
        codex_home: "/home/showcase/.codex",
        hook_path: "/home/showcase/.codex/hooks.json",
        enabled: true,
        available: true,
        activation: "verified",
        clients: [
          {
            id: "wsl_cli",
            label: "Codex CLI · WSL",
            verified: true,
            last_captured_at_us: Math.round(todayAt(0.55) * 1_000),
          },
        ],
        detail: null,
      },
    ],
  };
  state.activityOrigins = Object.fromEntries(seeds.map((seed) => [seed.id, seed.originId]));
  state.conversationRoutes = { "codex:showcase-release": 1 };
  state.nextProjectId = 4;
  state.nextEdgeId = 207;
  state.curationLogs = curationLogs;
  state.curationProposals = {};
  state.workItems = [
    work(1, 1, "수집 파이프라인 안정화", [1, 2], 80, 130),
    work(2, 2, "촉각 충돌 프로토타입", [3, 4], 500, 330),
  ];
  state.workEdges = [];
  state.workRevision = 1;
  state.nextProposalId = 1;
  state.nextWorkId = 3;
  state.nextWorkEdgeId = 1;
  return state;
}
