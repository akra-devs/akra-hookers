import { describe, expect, it, vi } from "vitest";

import {
  ApiError,
  createApiClient,
  type ActivityDetail,
  type ActivitySummary,
  type OriginSummary,
} from "./api";
import type { ActivityNodeData } from "./canvas";

const base = "http://127.0.0.1:4319";
const authorization = { Authorization: "Bearer capability" };
const json = (value: unknown, status = 200) =>
  new Response(JSON.stringify(value), {
    status,
    headers: { "Content-Type": "application/json" },
  });

describe("createApiClient", () => {
  it("encodes explicit all, Inbox, and project activity scopes", async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json([])));
    const client = createApiClient(base, "capability", fetcher);

    await client.activities({ scope: "all" });
    await client.activities({ scope: "inbox" });
    await client.activities({ scope: "project", projectId: 42 });
    await client.activities({ scope: "all" }, { limit: 20, afterId: 7 });

    expect(fetcher.mock.calls.map(([url]) => url)).toEqual([
      `${base}/v1/activities?scope=all`,
      `${base}/v1/activities?scope=inbox`,
      `${base}/v1/activities?scope=project&project_id=42`,
      `${base}/v1/activities?scope=all&limit=20&after_id=7`,
    ]);
    expect(fetcher).toHaveBeenNthCalledWith(
      1,
      `${base}/v1/activities?scope=all`,
      expect.objectContaining({ headers: authorization }),
    );
  });

  it("loads detail by immutable activity id", async () => {
    const response = { id: 7, technical: { session_id: "s", turn_id: "t" } };
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json(response)));
    const client = createApiClient(base, "capability", fetcher);

    await expect(client.activity(7)).resolves.toEqual(response);
    await expect(client.activity(7, { limit: 25, afterId: 6 })).resolves.toEqual(response);
    expect(fetcher).toHaveBeenCalledWith(
      `${base}/v1/activities/7`,
      expect.objectContaining({ headers: authorization }),
    );
    expect(fetcher).toHaveBeenNthCalledWith(
      2,
      `${base}/v1/activities/7?conversation_limit=25&conversation_after_id=6`,
      expect.objectContaining({ headers: authorization }),
    );
  });

  it("sends every project lifecycle request with exact bodies", async () => {
    const fetcher = vi.fn().mockImplementation((url, init) =>
      Promise.resolve(json(
        { id: 8, name: "새 이름" },
        url === `${base}/v1/projects` && init?.method === "POST" ? 201 : 200,
      ))
    );
    const client = createApiClient(base, "capability", fetcher);

    await client.projects();
    await client.createProject("새 프로젝트");
    await client.renameProject(8, "새 이름");
    await client.mergeProject(8, 9);

    expect(fetcher.mock.calls.map(([url, init]) => [url, init?.method])).toEqual([
      [`${base}/v1/projects`, undefined],
      [`${base}/v1/projects`, "POST"],
      [`${base}/v1/projects/8`, "PATCH"],
      [`${base}/v1/projects/8/merge`, "POST"],
    ]);
    expect(fetcher.mock.calls.map(([, init]) => init?.body)).toEqual([
      undefined,
      JSON.stringify({ name: "새 프로젝트" }),
      JSON.stringify({ name: "새 이름" }),
      JSON.stringify({ target_project_id: 9 }),
    ]);
  });

  it("sends every origin query and dedicated/shared transition", async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json([])));
    const client = createApiClient(base, "capability", fetcher);

    await client.origins();
    await client.projectOrigins(5);
    await client.configureOrigin(6, { mode: "shared", confirm: true });
    await client.configureOrigin(6, {
      mode: "dedicated",
      destination: { new_project_name: "한 프로젝트" },
      confirm: true,
    });

    expect(fetcher.mock.calls.map(([url]) => url)).toEqual([
      `${base}/v1/origins`,
      `${base}/v1/projects/5/origins`,
      `${base}/v1/origins/6/routing`,
      `${base}/v1/origins/6/routing`,
    ]);
    expect(fetcher.mock.calls[2]?.[1]?.body).toBe(
      JSON.stringify({ mode: "shared", confirm: true }),
    );
    expect(fetcher.mock.calls[3]?.[1]?.body).toBe(JSON.stringify({
      mode: "dedicated",
      destination: { new_project_name: "한 프로젝트" },
      confirm: true,
    }));
  });

  it("accepts recovered legacy origin provenance", async () => {
    const response: OriginSummary[] = [{
      id: 6,
      display_path: "C:\\legacy",
      kind: "directory",
      resolution_source: "legacy_resolved",
      setup_state: "unconfirmed",
      routing_mode: "shared",
      default_project_id: null,
      default_project_name: null,
      activity_count: 1,
      conversation_count: 1,
      recommended_mode: "dedicated",
    }];
    const client = createApiClient(
      base,
      "capability",
      vi.fn().mockResolvedValue(json(response)),
    );

    await expect(client.origins()).resolves.toEqual(response);
  });

  it("types recovered legacy activity-detail provenance", () => {
    const source: ActivityDetail["origin"]["resolution_source"] =
      "legacy_resolved";

    expect(source).toBe("legacy_resolved");
  });

  it("keeps explicit Inbox destination and future-route intent", async () => {
    const fetcher = vi.fn().mockResolvedValue(
      json({ activity_ids: [3, 7], project_id: null, future_route: "clear" }),
    );
    const client = createApiClient(base, "capability", fetcher);

    await client.assignActivities({
      activity_ids: [7, 3],
      destination: null,
      future_route: "clear",
    });

    expect(fetcher).toHaveBeenCalledWith(
      `${base}/v1/activity-assignments`,
      expect.objectContaining({
        method: "POST",
        headers: { ...authorization, "Content-Type": "application/json" },
        body: JSON.stringify({
          activity_ids: [7, 3],
          destination: null,
          future_route: "clear",
        }),
      }),
    );
  });

  it("preserves every canvas and provider endpoint", async () => {
    const fetcher = vi.fn().mockImplementation(() => Promise.resolve(json([])));
    const client = createApiClient(base, "capability", fetcher);

    await client.canvas();
    await client.canvasRevision();
    await client.edges();
    await client.clearCanvas();
    await client.deleteCanvasNode(4);
    await client.createCanvasEdge(4, 5);
    await client.deleteCanvasEdge(21);
    await client.updateCanvasPosition(4, { x: 12, y: 13 });
    await client.provider("codex");
    await client.setProviderEnabled("codex", false);

    expect(fetcher.mock.calls.map(([url, init]) => [url, init?.method])).toEqual([
      [`${base}/v1/canvas`, undefined],
      [`${base}/v1/canvas/revision`, undefined],
      [`${base}/v1/canvas/edges`, undefined],
      [`${base}/v1/canvas`, "DELETE"],
      [`${base}/v1/canvas/4`, "DELETE"],
      [`${base}/v1/canvas/edges`, "POST"],
      [`${base}/v1/canvas/edges/21`, "DELETE"],
      [`${base}/v1/canvas/4`, "PATCH"],
      [`${base}/v1/providers/codex`, undefined],
      [`${base}/v1/providers/codex`, "PATCH"],
    ]);
  });

  it.each([
    [401, "unauthorized", "Capability token is invalid"],
    [404, "not_found", "Activity was not found"],
    [409, "name_conflict", "Project name already exists"],
    [422, "invalid_request", "Choose a valid destination"],
  ])("exposes %i JSON errors without collapsing status", async (status, code, message) => {
    const client = createApiClient(
      base,
      "capability",
      vi.fn().mockResolvedValue(json({ code, message }, status)),
    );

    const error = await client.projects().catch((cause: unknown) => cause);
    expect(error).toBeInstanceOf(ApiError);
    expect(error).toMatchObject({ status, code, message, body: { code, message } });
  });
});

type ForbiddenSummaryKey = Extract<
  keyof ActivitySummary,
  "session_id" | "turn_id" | "cwd" | "submitted_cwd"
>;
type ForbiddenNodeKey = Extract<
  keyof ActivityNodeData,
  "sessionId" | "turnId" | "cwd" | "submittedCwd"
>;
type AssertNever<T extends never> = T;
type _SummaryHasNoTechnicalFields = AssertNever<ForbiddenSummaryKey>;
type _NodeHasNoTechnicalFields = AssertNever<ForbiddenNodeKey>;
type _TechnicalFieldsStayInDetail = ActivityDetail["technical"]["session_id" | "turn_id"];
