import type {
  ActivityAssignmentRequest,
  ActivityAssignmentResult,
  ActivityDetail,
  ActivityScope,
  ActivitySummary,
  ApiErrorBody,
  CanvasEdge,
  CanvasNode,
  CodexCaptureTarget,
  CodexCaptureClient,
  CollectorIntegration,
  OriginRoutingRequest,
  OriginSummary,
  ProjectSummary,
  ProviderIntegration,
} from "./api-contracts";

export type {
  ActivityAssignmentRequest,
  ActivityAssignmentResult,
  ActivityConversationTurn,
  ActivityDetail,
  ActivityKind,
  ActivityProject,
  ActivityResultSummary,
  ActivityScope,
  ActivitySummary,
  ActivityTime,
  ActivityTimeProvenance,
  ApiErrorBody,
  CanvasEdge,
  CanvasNode,
  CodexCaptureTarget,
  CodexCaptureClient,
  CollectorIntegration,
  FutureRoute,
  OriginRoutingRequest,
  OriginSummary,
  ProjectDestination,
  ProjectSummary,
  ProviderIntegration,
  ResultSummaryStatus,
} from "./api-contracts";

export class ApiError extends Error {
  readonly status: number;
  readonly code: string;
  readonly body: ApiErrorBody;

  constructor(status: number, body: ApiErrorBody) {
    super(body.message);
    this.name = "ApiError";
    this.status = status;
    this.code = body.code;
    this.body = body;
  }
}

export type ActivityVisibilityQuery = {
  includeSubagent?: boolean;
  includeInternal?: boolean;
};

type ActivityPageQuery = ActivityVisibilityQuery & {
  limit?: number;
  afterId?: number;
  order?: "oldest" | "newest";
};

type ConversationPageQuery = ActivityVisibilityQuery & {
  limit?: number;
  afterId?: number;
};

export type ApiClient = {
  activities(
    scope: ActivityScope,
    page?: ActivityPageQuery,
  ): Promise<ActivitySummary[]>;
  activityCount(scope: ActivityScope, visibility?: ActivityVisibilityQuery): Promise<number>;
  activity(
    activityId: number,
    page?: ConversationPageQuery,
  ): Promise<ActivityDetail>;
  projects(visibility?: ActivityVisibilityQuery): Promise<ProjectSummary[]>;
  createProject(name: string): Promise<ProjectSummary>;
  renameProject(projectId: number, name: string): Promise<ProjectSummary>;
  mergeProject(sourceProjectId: number, targetProjectId: number): Promise<ProjectSummary>;
  origins(): Promise<OriginSummary[]>;
  projectOrigins(projectId: number): Promise<OriginSummary[]>;
  configureOrigin(originId: number, request: OriginRoutingRequest): Promise<OriginSummary>;
  assignActivities(request: ActivityAssignmentRequest): Promise<ActivityAssignmentResult>;
  canvas(): Promise<CanvasNode[]>;
  canvasRevision(): Promise<number>;
  clearCanvas(): Promise<void>;
  deleteCanvasNode(nodeId: number): Promise<void>;
  createCanvasEdge(sourceNodeId: number, targetNodeId: number): Promise<void>;
  deleteCanvasEdge(edgeId: number): Promise<void>;
  edges(): Promise<CanvasEdge[]>;
  updateCanvasPosition(nodeId: number, position: { x: number; y: number }): Promise<void>;
  setProviderEnabled(provider: string, enabled: boolean): Promise<void>;
  setProviderTargetEnabled(provider: string, targetId: string, enabled: boolean): Promise<void>;
  configureCollector(endpoint: string, token?: string): Promise<void>;
  verifyCollector(): Promise<void>;
  provider(provider: string): Promise<ProviderIntegration>;
};

type Fetch = typeof fetch;
type Method = "DELETE" | "PATCH" | "POST" | "PUT";

export function createApiClient(
  baseUrl: string,
  token: string,
  fetcher: Fetch = fetch,
): ApiClient {
  async function request<T>(
    path: string,
    method?: Method,
    body?: unknown,
  ): Promise<T> {
    const headers: Record<string, string> = {
      Authorization: `Bearer ${token}`,
    };
    if (body !== undefined) {
      headers["Content-Type"] = "application/json";
    }
    const response = await fetcher(`${baseUrl}${path}`, {
      ...(method ? { method } : {}),
      headers,
      ...(body !== undefined ? { body: JSON.stringify(body) } : {}),
    });
    if (!response.ok) {
      throw await apiError(response);
    }
    if (response.status === 204 || response.headers.get("Content-Length") === "0") {
      return undefined as T;
    }
    return response.json() as Promise<T>;
  }

  return {
    activities: (scope, page) =>
      request<ActivitySummary[]>(activityPath(scope, page)),
    activityCount: async (scope, visibility) => {
      const result = await request<{ count: number }>(activityCountPath(scope, visibility));
      return result.count;
    },
    activity: (activityId, page) =>
      request<ActivityDetail>(conversationPath(activityId, page)),
    projects: (visibility) =>
      request<ProjectSummary[]>(appendActivityVisibility("/v1/projects", visibility)),
    createProject: (name) => request<ProjectSummary>("/v1/projects", "POST", { name }),
    renameProject: (projectId, name) =>
      request<ProjectSummary>(`/v1/projects/${projectId}`, "PATCH", { name }),
    mergeProject: (sourceProjectId, targetProjectId) =>
      request<ProjectSummary>(`/v1/projects/${sourceProjectId}/merge`, "POST", {
        target_project_id: targetProjectId,
      }),
    origins: () => request<OriginSummary[]>("/v1/origins"),
    projectOrigins: (projectId) =>
      request<OriginSummary[]>(`/v1/projects/${projectId}/origins`),
    configureOrigin: (originId, command) =>
      request<OriginSummary>(`/v1/origins/${originId}/routing`, "PATCH", command),
    assignActivities: (command) =>
      request<ActivityAssignmentResult>("/v1/activity-assignments", "POST", command),
    canvas: () => request<CanvasNode[]>("/v1/canvas"),
    canvasRevision: async () => {
      const result = await request<{ revision: number }>("/v1/canvas/revision");
      return result.revision;
    },
    clearCanvas: () => request<void>("/v1/canvas", "DELETE"),
    edges: () => request<CanvasEdge[]>("/v1/canvas/edges"),
    createCanvasEdge: (sourceNodeId, targetNodeId) =>
      request<void>("/v1/canvas/edges", "POST", {
        source_node_id: sourceNodeId,
        target_node_id: targetNodeId,
      }),
    deleteCanvasEdge: (edgeId) =>
      request<void>(`/v1/canvas/edges/${edgeId}`, "DELETE"),
    deleteCanvasNode: (nodeId) => request<void>(`/v1/canvas/${nodeId}`, "DELETE"),
    updateCanvasPosition: (nodeId, position) =>
      request<void>(`/v1/canvas/${nodeId}`, "PATCH", {
        position_x: position.x,
        position_y: position.y,
      }),
    setProviderEnabled: (provider, enabled) =>
      request<void>(`/v1/providers/${encodeURIComponent(provider)}`, "PATCH", { enabled }),
    setProviderTargetEnabled: (provider, targetId, enabled) =>
      request<void>(
        `/v1/providers/${encodeURIComponent(provider)}/targets/${encodeURIComponent(targetId)}`,
        "PATCH",
        { enabled },
      ),
    configureCollector: (endpoint, collectorToken) =>
      request<void>("/v1/providers/codex/collector", "PUT", {
        endpoint,
        ...(collectorToken === undefined ? {} : { token: collectorToken }),
      }),
    verifyCollector: () =>
      request<void>("/v1/providers/codex/collector/verify", "POST"),
    provider: (provider) =>
      request<ProviderIntegration>(`/v1/providers/${encodeURIComponent(provider)}`),
  };
}

function activityPath(
  scope: ActivityScope,
  page?: ActivityPageQuery,
): string {
  let path: string;
  switch (scope.scope) {
    case "all":
      path = "/v1/activities?scope=all";
      break;
    case "inbox":
      path = "/v1/activities?scope=inbox";
      break;
    case "project":
      path = `/v1/activities?scope=project&project_id=${encodeURIComponent(scope.projectId)}`;
      break;
  }
  return appendPage(path, page, "after_id");
}

function conversationPath(
  activityId: number,
  page?: ConversationPageQuery,
): string {
  return appendPage(
    `/v1/activities/${activityId}`,
    page,
    "conversation_after_id",
    "conversation_limit",
  );
}

function activityCountPath(
  scope: ActivityScope,
  visibility?: ActivityVisibilityQuery,
): string {
  return activityPath(scope, visibility)
    .replace("/v1/activities?", "/v1/activities/count?");
}

function appendPage(
  path: string,
  page: ActivityPageQuery | ConversationPageQuery | undefined,
  cursorName: string,
  limitName = "limit",
): string {
  if (!page) return path;
  const parameters = new URLSearchParams();
  if (page.limit !== undefined) parameters.set(limitName, String(page.limit));
  if (page.afterId !== undefined) parameters.set(cursorName, String(page.afterId));
  if ("order" in page && page.order !== undefined) parameters.set("order", page.order);
  appendVisibilityParameters(parameters, page);
  const query = parameters.toString();
  return query.length === 0
    ? path
    : `${path}${path.includes("?") ? "&" : "?"}${query}`;
}

function appendActivityVisibility(
  path: string,
  visibility: ActivityVisibilityQuery | undefined,
): string {
  if (!visibility) return path;
  const parameters = new URLSearchParams();
  appendVisibilityParameters(parameters, visibility);
  const query = parameters.toString();
  return query ? `${path}?${query}` : path;
}

function appendVisibilityParameters(
  parameters: URLSearchParams,
  visibility: ActivityVisibilityQuery,
) {
  if (visibility.includeSubagent !== undefined) {
    parameters.set("include_subagent", String(visibility.includeSubagent));
  }
  if (visibility.includeInternal !== undefined) {
    parameters.set("include_internal", String(visibility.includeInternal));
  }
}

async function apiError(response: Response): Promise<ApiError> {
  const fallback: ApiErrorBody = {
    code: `http_${response.status}`,
    message: `API request failed: ${response.status}`,
  };
  try {
    const body: unknown = await response.json();
    if (
      typeof body === "object"
      && body !== null
      && "code" in body
      && "message" in body
      && typeof body.code === "string"
      && typeof body.message === "string"
    ) {
      return new ApiError(response.status, { code: body.code, message: body.message });
    }
  } catch {
    // Empty and non-JSON error responses use the typed HTTP fallback.
  }
  return new ApiError(response.status, fallback);
}
