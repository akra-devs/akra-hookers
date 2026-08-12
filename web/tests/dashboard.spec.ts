import { expect, test as base, type Page } from "@playwright/test";

import { FixtureApi, fixtureApiUrl, fixtureToken, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [
    async ({ page }, use) => {
      await use(await installFixtureApi(page));
    },
    { auto: true },
  ],
});

test("fixture setup rejects unauthenticated mutation and accepts authenticated setup", async ({ api }) => {
  const url = new URL(`${fixtureApiUrl}/v1/origins/2/routing`);
  const before = api.state.origins[1]?.routing_mode;
  const denied = await api.dispatch("PATCH", url, {}, { mode: "shared", confirm: true });
  expect(denied.status).toBe(401);
  expect(api.state.origins[1]?.routing_mode).toBe(before);

  const accepted = await api.dispatch(
    "PATCH",
    url,
    auth(),
    { mode: "shared", confirm: true },
  );
  expect(accepted.status).toBe(200);
  expect(api.state.origins[1]).toMatchObject({
    setup_state: "confirmed",
    routing_mode: "shared",
  });
});

test("canvas visibility independently filters subagent and Codex internal nodes", async ({
  page,
  api,
}) => {
  const template = api.state.activities[0]!;
  api.state.activities.push(
    {
      ...structuredClone(template),
      id: 3,
      activity_kind: "subagent",
      prompt: "Subagent started: reviewer",
      conversation_index: 3,
      conversation_total: 4,
    },
    {
      ...structuredClone(template),
      id: 4,
      activity_kind: "internal",
      prompt: "Ambient suggestion evaluation",
      conversation_index: 4,
      conversation_total: 4,
    },
  );
  for (const detail of Object.values(api.state.details)) {
    const turn = detail.conversation[0]!;
    detail.conversation.push(
      {
        ...structuredClone(turn),
        id: 3,
        activity_kind: "subagent",
        prompt: "Subagent started: reviewer",
        selected: false,
      },
      {
        ...structuredClone(turn),
        id: 4,
        activity_kind: "internal",
        prompt: "Ambient suggestion evaluation",
        selected: false,
      },
    );
    detail.conversation_total = 4;
  }
  api.state.canvasNodes.push(
    { id: 13, activity_event_id: 3, position_x: 720, position_y: 120 },
    { id: 14, activity_event_id: 4, position_x: 720, position_y: 360 },
  );
  api.state.canvasEdges.push(
    { id: 22, source_node_id: 11, target_node_id: 13 },
    { id: 23, source_node_id: 13, target_node_id: 14 },
  );

  await page.goto("/");

  const subagentToggle = page.getByRole("checkbox", { name: /Subagent activity/ });
  const internalToggle = page.getByRole("checkbox", { name: /Codex internal activity/ });
  await expect(subagentToggle).toBeChecked();
  await expect(internalToggle).not.toBeChecked();
  await expect(page.getByTestId("activity-node-3")).toBeVisible();
  await expect(page.getByTestId("activity-node-4")).toHaveCount(0);
  const firstProjectCount = page.locator(".rail-projects li").first().locator("small");
  await expect(firstProjectCount).toHaveText("2");

  await subagentToggle.uncheck();
  await expect(page.getByTestId("activity-node-3")).toHaveCount(0);
  await expect(firstProjectCount).toHaveText("1");
  await page.getByTestId("activity-node-1").click();
  const detailPanel = page.getByTestId("activity-detail-panel");
  await expect(detailPanel.locator(".activity-detail__turn")).toHaveCount(2);
  await expect(detailPanel).not.toContainText("Subagent started: reviewer");
  await internalToggle.check();
  await expect(page.getByTestId("activity-node-4")).toBeVisible();
  await expect(firstProjectCount).toHaveText("2");
  await expect(detailPanel).toContainText("Ambient suggestion evaluation");
  expect(api.state.activities.map(({ id }) => id)).toEqual([1, 2, 3, 4]);

  await page.reload();
  await expect(page.getByRole("checkbox", { name: /Subagent activity/ })).not.toBeChecked();
  await expect(page.getByRole("checkbox", { name: /Codex internal activity/ })).toBeChecked();
  await expect(page.getByTestId("activity-node-3")).toHaveCount(0);
  await expect(page.getByTestId("activity-node-4")).toBeVisible();
});

test("the newest bounded page discovers activity 101 and older pages load explicitly", async ({
  page,
  api,
}) => {
  const activity = api.state.activities[1]!;
  for (let id = 3; id <= 101; id += 1) {
    api.state.activities.push({
      ...structuredClone(activity),
      id,
      prompt: `prompt ${id}`,
      conversation_index: id,
      conversation_total: 101,
    });
    api.state.canvasNodes.push({
      id: 1000 + id,
      activity_event_id: id,
      position_x: id * 12,
      position_y: id * 8,
    });
  }
  await page.goto("/");

  await expect(page.getByTestId("activity-node-101")).toBeVisible();
  await expect(page.getByTestId("activity-node-1")).toHaveCount(0);
  const olderResponse = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/v1/activities"
      && url.searchParams.get("order") === "newest"
      && url.searchParams.get("after_id") === "2";
  });
  await page.getByRole("button", { name: "이전 활동 불러오기" }).click();
  await olderResponse;

  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.locator("[data-testid^=activity-node-]")).toHaveCount(101);
  await expect(page.getByRole("button", { name: "이전 활동 불러오기" })).toHaveCount(0);

  const newest = {
    ...structuredClone(activity),
    id: 102,
    prompt: "prompt 102",
    conversation_index: 102,
    conversation_total: 102,
  };
  const newestNode = {
    id: 1102,
    activity_event_id: 102,
    position_x: 1224,
    position_y: 816,
  };
  const refreshedActivities = page.waitForResponse(async (response) => {
    const url = new URL(response.url());
    if (
      url.pathname !== "/v1/activities"
      || url.searchParams.get("order") !== "newest"
    ) return false;
    const body = await response.json() as Array<{ id: number }>;
    return body[0]?.id === 102;
  });
  const refreshedCanvas = page.waitForResponse((response) =>
    new URL(response.url()).pathname === "/v1/canvas"
  );
  api.state.activities.push(newest);
  api.state.canvasNodes.push(newestNode);
  await api.dispatch(
    "PATCH",
    new URL(`${fixtureApiUrl}/v1/canvas/${newestNode.id}`),
    auth(),
    { position_x: newestNode.position_x, position_y: newestNode.position_y },
  );
  await Promise.all([refreshedActivities, refreshedCanvas]);

  await expect(page.getByTestId("activity-node-102")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).toBeVisible();
});

test("fixture project rename propagates through the rendered selector", async ({ page }) => {
  await page.goto("/");
  const response = await browserRequest(
    page,
    "PATCH",
    "/v1/projects/1",
    { name: "새 프로젝트 이름" },
  );
  expect(response.status).toBe(200);
  await page.reload();

  await expect(page.getByRole("option", { name: "새 프로젝트 이름" })).toBeAttached();
});

test("fixture Inbox assignment changes scoped query state", async ({ page, api }) => {
  const response = await browserRequest(page, "POST", "/v1/activity-assignments", {
    activity_ids: [1],
    destination: null,
    future_route: "unchanged",
  });
  expect(response.status).toBe(200);
  const inbox = await api.dispatch(
    "GET",
    new URL(`${fixtureApiUrl}/v1/activities?scope=inbox`),
    auth(),
  );
  expect(inbox.body).toEqual(expect.arrayContaining([
    expect.objectContaining({ id: 1, project: null }),
  ]));
});

test("fixture detail preserves technical metadata only in detail", async ({ api }) => {
  const detail = await api.dispatch(
    "GET",
    new URL(`${fixtureApiUrl}/v1/activities/1`),
    auth(),
  );
  expect(detail.body).toMatchObject({
    technical: { session_id: "fixture-session", turn_id: "turn-1" },
  });
  expect(api.state.activities[0]).not.toHaveProperty("technical");
});

test("fixture canvas mutation persists exact coordinates", async ({ page, api }) => {
  const response = await browserRequest(page, "PATCH", "/v1/canvas/11", {
    position_x: 321,
    position_y: 654,
  });
  expect(response.status).toBe(204);
  expect(api.state.canvasNodes[0]).toMatchObject({ position_x: 321, position_y: 654 });
});

test("fixture CJK project creation preserves Unicode spelling", async ({ page, api }) => {
  const response = await browserRequest(page, "POST", "/v1/projects", {
    name: "한글 프로젝트",
  });
  expect(response.status).toBe(201);
  expect(api.state.projects).toEqual(expect.arrayContaining([
    expect.objectContaining({ name: "한글 프로젝트" }),
  ]));
});

test("fixture project merge retargets saved conversation routes", async ({ api }) => {
  const created = await api.dispatch(
    "POST",
    new URL(`${fixtureApiUrl}/v1/projects`),
    auth(),
    { name: "병합 대상" },
  );
  expect(created.status).toBe(201);
  api.state.conversationRoutes["codex:fixture-session"] = 1;

  const merged = await api.dispatch(
    "POST",
    new URL(`${fixtureApiUrl}/v1/projects/1/merge`),
    auth(),
    { target_project_id: 2 },
  );
  expect(merged.status).toBe(200);
  expect(api.state.conversationRoutes["codex:fixture-session"]).toBe(2);
});

test("fixture dedicated origin transition moves matching activity details", async ({ api }) => {
  api.state.activityOrigins[2] = 2;
  const projectId = api.state.nextProjectId;
  const routed = await api.dispatch(
    "PATCH",
    new URL(`${fixtureApiUrl}/v1/origins/1/routing`),
    auth(),
    {
      mode: "dedicated",
      destination: { new_project_name: "연결된 프로젝트" },
      confirm: true,
    },
  );
  expect(routed.status).toBe(200);
  expect(api.state.activities).toEqual(expect.arrayContaining([
    expect.objectContaining({ project: { id: projectId, name: "연결된 프로젝트" } }),
    expect.objectContaining({ id: 2, project: null }),
  ]));
  expect(api.state.details[1]?.project).toEqual({
    id: projectId,
    name: "연결된 프로젝트",
  });
  expect(api.state.details[2]?.project).toBeNull();
  expect(api.state.details[2]?.conversation).toEqual(expect.arrayContaining([
    expect.objectContaining({
      id: 1,
      project: { id: projectId, name: "연결된 프로젝트" },
    }),
  ]));
});

test("fixture unexpected endpoint fails immediately with method and path", async ({ api }) => {
  await expect(api.dispatch(
    "GET",
    new URL(`${fixtureApiUrl}/v1/unregistered`),
    auth(),
  )).rejects.toThrow("Unexpected API request: GET /v1/unregistered");
});

test("fixture mirrors unavailable Codex capture target contracts", async ({ api }) => {
  const wslTarget = api.state.provider.targets.find(({ id }) => id === "wsl:Ubuntu")!;
  wslTarget.available = false;
  wslTarget.enabled = false;
  const targetResult = await api.dispatch(
    "PATCH",
    new URL(`${fixtureApiUrl}/v1/providers/codex/targets/wsl%3AUbuntu`),
    auth(),
    { enabled: true },
  );
  expect(targetResult).toMatchObject({
    status: 422,
    body: { code: "codex_target_unavailable" },
  });
  expect(wslTarget.enabled).toBe(false);

  api.state.provider.enabled = false;
  for (const target of api.state.provider.targets) {
    target.available = false;
    target.enabled = false;
  }
  const globalResult = await api.dispatch(
    "PATCH",
    new URL(`${fixtureApiUrl}/v1/providers/codex`),
    auth(),
    { enabled: true },
  );
  expect(globalResult).toMatchObject({
    status: 422,
    body: { code: "codex_target_unavailable" },
  });
  expect(api.state.provider.enabled).toBe(false);
  expect(api.state.provider.targets.every(({ enabled }) => !enabled)).toBe(true);
});

async function browserRequest(
  page: Page,
  method: "PATCH" | "POST",
  path: string,
  body: unknown,
): Promise<{ status: number; body: unknown }> {
  return page.evaluate(async ({ url, token, method, body }) => {
    const response = await fetch(url, {
      method,
      headers: {
        Authorization: `Bearer ${token}`,
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    });
    const text = await response.text();
    return {
      status: response.status,
      body: text ? JSON.parse(text) : null,
    };
  }, { url: `${fixtureApiUrl}${path}`, token: fixtureToken, method, body });
}

function auth(): Record<string, string> {
  return { authorization: `Bearer ${fixtureToken}` };
}
