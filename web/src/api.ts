export type Activity = {
  id: number;
  provider: string;
  session_id: string;
  turn_id: string;
  prompt: string;
};

export type Project = {
  identity: string;
  display_path: string;
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

export type ApiClient = {
  activities(project?: string): Promise<Activity[]>;
  canvas(): Promise<CanvasNode[]>;
  clearCanvas(): Promise<void>;
  deleteCanvasNode(nodeId: number): Promise<void>;
  createCanvasEdge(sourceNodeId: number, targetNodeId: number): Promise<void>;
  edges(): Promise<CanvasEdge[]>;
  updateCanvasPosition(nodeId: number, position: { x: number; y: number }): Promise<void>;
  setProviderEnabled(provider: string, enabled: boolean): Promise<void>;
  provider(provider: string): Promise<ProviderIntegration>;
  projects(): Promise<Project[]>;
};

type Fetch = typeof fetch;

export function createApiClient(baseUrl: string, token: string, fetcher: Fetch = fetch): ApiClient {
  async function get<T>(path: string): Promise<T> {
    const response = await fetcher(`${baseUrl}${path}`, {
      headers: { Authorization: `Bearer ${token}` },
    });
    if (!response.ok) {
      throw new Error(`API request failed: ${response.status}`);
    }
    return response.json() as Promise<T>;
  }

  async function send(path: string, method: "DELETE" | "PATCH" | "POST", body?: unknown): Promise<void> {
    const response = await fetcher(`${baseUrl}${path}`, {
      method,
      headers: { Authorization: `Bearer ${token}`, "Content-Type": "application/json" },
      body: body ? JSON.stringify(body) : undefined,
    });
    if (!response.ok) {
      throw new Error(`API request failed: ${response.status}`);
    }
  }

  return {
    activities: (project) => get<Activity[]>(
      project ? `/v1/activities?project=${encodeURIComponent(project)}` : "/v1/activities",
    ),
    canvas: () => get<CanvasNode[]>("/v1/canvas"),
    clearCanvas: () => send("/v1/canvas", "DELETE"),
    edges: () => get<CanvasEdge[]>("/v1/canvas/edges"),
    createCanvasEdge: (sourceNodeId, targetNodeId) => send("/v1/canvas/edges", "POST", {
      source_node_id: sourceNodeId,
      target_node_id: targetNodeId,
    }),
    deleteCanvasNode: (nodeId) => send(`/v1/canvas/${nodeId}`, "DELETE"),
    updateCanvasPosition: (nodeId, position) => send(`/v1/canvas/${nodeId}`, "PATCH", {
      position_x: position.x,
      position_y: position.y,
    }),
    setProviderEnabled: (provider, enabled) => send(`/v1/providers/${provider}`, "PATCH", { enabled }),
    provider: (provider) => get<ProviderIntegration>(`/v1/providers/${provider}`),
    projects: () => get<Project[]>("/v1/projects"),
  };
}
