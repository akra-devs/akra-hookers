import type { Page, Route } from "@playwright/test";

import type { ActivityAssignmentRequest, OriginRoutingRequest } from "../../src/api";
import { FixtureModel } from "./model";
import { createFixtureState, type FixtureState } from "./state";

export const fixtureApiUrl = "http://127.0.0.1:4173";
export const fixtureToken = "fixture-token";

type FixtureResponse = {
  status: number;
  body?: unknown;
};

type DeferredDetail = {
  requested: Promise<void>;
  released: Promise<void>;
  resolveRequested: () => void;
  resolveRelease: () => void;
};

export class FixtureApi {
  private readonly model: FixtureModel;
  private readonly deferredDetails = new Map<number, DeferredDetail[]>();
  private canvasRevision = 0;

  constructor(state: FixtureState = createFixtureState()) {
    this.model = new FixtureModel(state);
  }

  get state(): FixtureState {
    return this.model.state;
  }

  deferNextDetail(activityId: number): { requested: Promise<void>; release: () => void } {
    let resolveRequested!: () => void;
    let resolveRelease!: () => void;
    const deferred: DeferredDetail = {
      requested: new Promise<void>((resolve) => { resolveRequested = resolve; }),
      released: new Promise<void>((resolve) => { resolveRelease = resolve; }),
      resolveRequested,
      resolveRelease,
    };
    this.deferredDetails.set(activityId, [
      ...(this.deferredDetails.get(activityId) ?? []),
      deferred,
    ]);
    return { requested: deferred.requested, release: deferred.resolveRelease };
  }

  async dispatch(
    method: string,
    url: URL,
    headers: Record<string, string>,
    body?: unknown,
  ): Promise<FixtureResponse> {
    if (method === "OPTIONS") {
      return { status: 204 };
    }
    if (headers.authorization !== `Bearer ${fixtureToken}`) {
      return error(401, "unauthorized", "Capability token is invalid");
    }
    const path = url.pathname;
    if (method === "GET" && path === "/v1/activities") {
      return { status: 200, body: this.activities(url) };
    }
    if (method === "GET" && path === "/v1/activities/count") {
      return { status: 200, body: { count: this.activities(url).length } };
    }
    const activityId = match(path, /^\/v1\/activities\/(\d+)$/);
    if (method === "GET" && activityId !== null) {
      const deferred = this.deferredDetails.get(activityId)?.shift();
      if (deferred) {
        deferred.resolveRequested();
        await deferred.released;
      }
      const detail = this.state.details[activityId];
      return detail
        ? { status: 200, body: this.detail(detail, url) }
        : error(404, "not_found", "Activity was not found");
    }
    if (method === "DELETE" && activityId !== null) {
      if (!this.state.details[activityId]) {
        return error(404, "not_found", "Activity was not found");
      }
      const removedNodeIds = new Set(
        this.state.canvasNodes
          .filter((node) => node.activity_event_id === activityId)
          .map((node) => node.id),
      );
      this.state.activities = this.state.activities.filter((activity) => activity.id !== activityId);
      delete this.state.details[activityId];
      delete this.state.activityOrigins[activityId];
      this.state.canvasNodes = this.state.canvasNodes.filter(
        (node) => node.activity_event_id !== activityId,
      );
      this.state.canvasEdges = this.state.canvasEdges.filter(
        (edge) => !removedNodeIds.has(edge.source_node_id) && !removedNodeIds.has(edge.target_node_id),
      );
      for (const [index, activity] of this.state.activities.entries()) {
        activity.conversation_index = index + 1;
        activity.conversation_total = this.state.activities.length;
      }
      for (const detail of Object.values(this.state.details)) {
        detail.conversation = detail.conversation.filter((turn) => turn.id !== activityId);
        detail.conversation_index = this.state.activities.findIndex(
          (activity) => activity.id === detail.id,
        ) + 1;
        detail.conversation_total = Math.max(0, detail.conversation_total - 1);
        detail.origin.activity_count = Math.max(0, detail.origin.activity_count - 1);
      }
      this.model.syncCanvasState();
      this.canvasRevision += 1;
      return { status: 204 };
    }
    if (path === "/v1/projects" && method === "GET") {
      return {
        status: 200,
        body: this.state.projects.map((project) => ({
          ...project,
          activity_count: this.state.activities.filter((activity) =>
            activity.project?.id === project.id
            && this.activityVisible(activity, url)).length,
        })),
      };
    }
    if (path === "/v1/projects" && method === "POST") {
      return { status: 201, body: this.model.createProject(named(body)) };
    }
    const projectId = match(path, /^\/v1\/projects\/(\d+)$/);
    if (method === "PATCH" && projectId !== null) {
      return { status: 200, body: this.model.renameProject(projectId, named(body)) };
    }
    const mergeId = match(path, /^\/v1\/projects\/(\d+)\/merge$/);
    if (method === "POST" && mergeId !== null) {
      const target = numeric(body, "target_project_id");
      return { status: 200, body: this.model.mergeProject(mergeId, target) };
    }
    if (path === "/v1/origins" && method === "GET") {
      return {
        status: 200,
        body: this.state.origins.map((origin) => ({
          ...origin,
          activity_count: this.state.activities.filter((activity) =>
            this.state.activityOrigins[activity.id] === origin.id
            && this.activityVisible(activity, url)).length,
        })),
      };
    }
    const projectOrigins = match(path, /^\/v1\/projects\/(\d+)\/origins$/);
    if (method === "GET" && projectOrigins !== null) {
      return {
        status: 200,
        body: this.state.origins.filter((origin) => origin.default_project_id === projectOrigins),
      };
    }
    const originId = match(path, /^\/v1\/origins\/(\d+)\/routing$/);
    if (method === "PATCH" && originId !== null) {
      return { status: 200, body: this.model.configureOrigin(originId, body as OriginRoutingRequest) };
    }
    if (path === "/v1/activity-assignments" && method === "POST") {
      return { status: 200, body: this.model.assign(body as ActivityAssignmentRequest) };
    }
    if (path === "/v1/canvas" && method === "GET") {
      return { status: 200, body: this.state.canvasNodes };
    }
    if (path === "/v1/canvas/revision" && method === "GET") {
      return { status: 200, body: { revision: this.canvasRevision } };
    }
    if (path === "/v1/canvas" && method === "DELETE") {
      this.state.canvasNodes = [];
      this.state.canvasEdges = [];
      this.model.syncCanvasState();
      this.canvasRevision += 1;
      return { status: 204 };
    }
    if (path === "/v1/canvas/edges" && method === "GET") {
      return { status: 200, body: this.state.canvasEdges };
    }
    if (path === "/v1/canvas/edges" && method === "POST") {
      this.state.canvasEdges.push({
        id: this.state.nextEdgeId++,
        source_node_id: numeric(body, "source_node_id"),
        target_node_id: numeric(body, "target_node_id"),
      });
      this.canvasRevision += 1;
      return { status: 201 };
    }
    const edgeId = match(path, /^\/v1\/canvas\/edges\/(\d+)$/);
    if (edgeId !== null && method === "DELETE") {
      this.state.canvasEdges = this.state.canvasEdges.filter((edge) => edge.id !== edgeId);
      this.canvasRevision += 1;
      return { status: 204 };
    }
    const canvasId = match(path, /^\/v1\/canvas\/(\d+)$/);
    if (canvasId !== null && method === "DELETE") {
      this.state.canvasNodes = this.state.canvasNodes.filter((node) => node.id !== canvasId);
      this.state.canvasEdges = this.state.canvasEdges.filter(
        (edge) => edge.source_node_id !== canvasId && edge.target_node_id !== canvasId,
      );
      this.model.syncCanvasState();
      this.canvasRevision += 1;
      return { status: 204 };
    }
    if (canvasId !== null && method === "PATCH") {
      const node = required(this.state.canvasNodes.find((candidate) => candidate.id === canvasId));
      node.position_x = numeric(body, "position_x");
      node.position_y = numeric(body, "position_y");
      this.canvasRevision += 1;
      return { status: 204 };
    }
    const providerTarget = /^\/v1\/providers\/([^/]+)\/targets\/([^/]+)$/.exec(path);
    if (providerTarget !== null && method === "PATCH") {
      const provider = decodeURIComponent(providerTarget[1] ?? "");
      const targetId = decodeURIComponent(providerTarget[2] ?? "");
      if (provider !== "codex") {
        return error(404, "not_found", "Provider was not found");
      }
      const target = this.state.provider.targets.find(({ id }) => id === targetId);
      if (!target) {
        return error(404, "not_found", "Codex capture target was not found");
      }
      if (!target.available) {
        return error(
          422,
          "codex_target_unavailable",
          "Codex capture target is unavailable",
        );
      }
      target.enabled = boolean(body, "enabled");
      target.activation = target.enabled ? "awaiting_capture" : "disabled";
      target.clients = target.clients.map((client) => ({
        ...client,
        verified: false,
        last_captured_at_us: null,
      }));
      this.state.provider.enabled = this.state.provider.targets.some(
        (candidate) => candidate.enabled,
      );
      return { status: 204 };
    }
    if (path === "/v1/providers/codex/collector" && method === "PUT") {
      const endpoint = text(body, "endpoint");
      const url = new URL(endpoint);
      const remote = url.protocol === "https:";
      const token = optionalText(body, "token");
      this.state.provider.collector = {
        ...this.state.provider.collector,
        mode: remote ? "remote" : "local",
        endpoint: url.origin,
        configured: true,
        token_configured: remote
          ? token === undefined ? this.state.provider.collector.token_configured : true
          : false,
        connected: !remote,
        last_error: null,
      };
      return { status: 204 };
    }
    if (path === "/v1/providers/codex/collector/verify" && method === "POST") {
      this.state.provider.collector.connected = true;
      this.state.provider.collector.last_error = null;
      return { status: 204 };
    }
    if (path === "/v1/providers/codex/prompt-summaries" && method === "PUT") {
      const mode = text(body, "mode");
      if (mode !== "off" && mode !== "smart") {
        return error(422, "invalid_prompt_summary_mode", "Prompt summary mode must be off or smart.");
      }
      this.state.provider.prompt_summary_mode = mode;
      return { status: 204 };
    }
    const provider = matchText(path, /^\/v1\/providers\/([^/]+)$/);
    if (provider !== null && method === "GET") {
      return provider === "codex"
        ? { status: 200, body: this.state.provider }
        : error(404, "not_found", "Provider was not found");
    }
    if (provider !== null && method === "PATCH") {
      if (provider !== "codex") {
        return error(404, "not_found", "Provider was not found");
      }
      const enabled = boolean(body, "enabled");
      const availableTargets = this.state.provider.targets.filter(
        (target) => target.available,
      );
      if (enabled && availableTargets.length === 0) {
        return error(
          422,
          "codex_target_unavailable",
          "No available Codex installations were detected",
        );
      }
      this.state.provider.enabled = enabled;
      for (const target of availableTargets) {
        target.enabled = enabled;
        target.activation = enabled ? "awaiting_capture" : "disabled";
        target.clients = target.clients.map((client) => ({
          ...client,
          verified: false,
          last_captured_at_us: null,
        }));
      }
      return { status: 204 };
    }
    throw new Error(`Unexpected API request: ${method} ${path}`);
  }

  private activities(url: URL) {
    const scope = url.searchParams.get("scope");
    let activities;
    if (scope === "all") {
      activities = this.state.activities;
    } else if (scope === "inbox") {
      activities = this.state.activities.filter((activity) => activity.project === null);
    } else if (scope === "project") {
      const projectId = Number(url.searchParams.get("project_id"));
      activities = this.state.activities.filter((activity) => activity.project?.id === projectId);
    } else {
      throw new Error(`Unexpected API request: GET ${url.pathname}${url.search}`);
    }
    activities = activities.filter((activity) => this.activityVisible(activity, url));
    const ordered = url.searchParams.get("order") === "newest"
      ? [...activities].reverse()
      : [...activities];
    const cursor = Number(url.searchParams.get("after_id"));
    const start = Number.isInteger(cursor) && cursor > 0
      ? ordered.findIndex(({ id }) => id === cursor) + 1
      : 0;
    const limit = Number(url.searchParams.get("limit")) || ordered.length;
    return ordered.slice(start, start + limit);
  }

  private detail(detail: FixtureState["details"][number], url: URL) {
    const conversation = detail.conversation.filter((turn) =>
      this.activityKindVisible(turn.activity_kind, url));
    const hiddenTotal = detail.conversation.filter((turn) =>
      !this.activityKindVisible(turn.activity_kind, url)).length;
    const conversationTotal = Math.max(0, detail.conversation_total - hiddenTotal);
    const cursor = Number(url.searchParams.get("conversation_after_id"));
    const cursorStart = Number.isInteger(cursor) && cursor > 0
      ? conversation.findIndex(({ id }) => id === cursor) + 1
      : 0;
    const requestedOffset = Number(url.searchParams.get("conversation_offset"));
    const start = url.searchParams.has("conversation_offset")
      && Number.isInteger(requestedOffset) && requestedOffset >= 0
      ? requestedOffset
      : cursorStart;
    const limit = Number(url.searchParams.get("conversation_limit")) || conversation.length;
    const page = conversation.slice(start, start + limit);
    return {
      ...detail,
      conversation: page,
      conversation_total: conversationTotal,
      conversation_has_more: start + page.length < conversationTotal,
    };
  }

  private activityKindVisible(activityKind: string, url: URL) {
    if (activityKind === "subagent") {
      return url.searchParams.get("include_subagent") !== "false";
    }
    if (activityKind === "internal") {
      return url.searchParams.get("include_internal") !== "false";
    }
    return true;
  }

  private activityVisible(activity: FixtureState["activities"][number], url: URL) {
    return this.activityKindVisible(activity.activity_kind, url)
      && this.activityPeriodVisible(activity.time.value, url);
  }

  private activityPeriodVisible(value: string | null, url: URL) {
    const period = url.searchParams.get("period") ?? "all";
    if (period === "all") return true;
    const hours = {
      day: 24,
      week: 24 * 7,
      month: 24 * 30,
      quarter: 24 * 90,
    }[period];
    if (hours === undefined || value === null) return false;
    const capturedAt = Date.parse(value);
    return Number.isFinite(capturedAt) && capturedAt >= Date.now() - hours * 60 * 60 * 1_000;
  }

}

export async function installFixtureApi(page: Page): Promise<FixtureApi> {
  const api = new FixtureApi();
  await page.route("**/v1/**", (route) => fulfill(route, api));
  return api;
}

async function fulfill(route: Route, api: FixtureApi): Promise<void> {
  const request = route.request();
  const text = request.postData();
  const body: unknown = text ? JSON.parse(text) : undefined;
  const response = await api.dispatch(request.method(), new URL(request.url()), request.headers(), body);
  const headers = {
    "Access-Control-Allow-Origin": "*",
    "Access-Control-Allow-Headers": "Authorization, Content-Type",
    "Access-Control-Allow-Methods": "GET, POST, PUT, PATCH, DELETE, OPTIONS",
    "Access-Control-Allow-Private-Network": "true",
  };
  await route.fulfill(response.body === undefined
    ? { status: response.status, headers }
    : { status: response.status, headers, contentType: "application/json", body: JSON.stringify(response.body) });
}

function match(path: string, pattern: RegExp): number | null {
  const value = pattern.exec(path)?.[1];
  return value === undefined ? null : Number(value);
}

function matchText(path: string, pattern: RegExp): string | null {
  return pattern.exec(path)?.[1] ?? null;
}

function required<T>(value: T | undefined): T {
  if (value === undefined) throw new Error("Fixture state invariant failed");
  return value;
}

function record(body: unknown): Record<string, unknown> {
  if (typeof body !== "object" || body === null || Array.isArray(body)) {
    throw new Error("Fixture request body must be an object");
  }
  return body as Record<string, unknown>;
}

function named(body: unknown): string {
  const value = record(body).name;
  if (typeof value !== "string") throw new Error("Fixture request name must be a string");
  return value;
}

function numeric(body: unknown, key: string): number {
  const value = record(body)[key];
  if (typeof value !== "number") throw new Error(`Fixture request ${key} must be a number`);
  return value;
}

function boolean(body: unknown, key: string): boolean {
  const value = record(body)[key];
  if (typeof value !== "boolean") throw new Error(`Fixture request ${key} must be a boolean`);
  return value;
}

function text(body: unknown, key: string): string {
  const value = record(body)[key];
  if (typeof value !== "string") throw new Error(`Fixture request ${key} must be a string`);
  return value;
}

function optionalText(body: unknown, key: string): string | undefined {
  const value = record(body)[key];
  if (value === undefined) return undefined;
  if (typeof value !== "string") throw new Error(`Fixture request ${key} must be a string`);
  return value;
}

function error(status: number, code: string, message: string): FixtureResponse {
  return { status, body: { code, message } };
}
