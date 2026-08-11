import { existsSync, readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

import { expect, test as base, type Page } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [async ({ page }, use) => use(await installFixtureApi(page)), { auto: true }],
});

type CanvasSnapshot = { nodes: unknown[]; edges: unknown[] };

function canvas(api: FixtureApi): CanvasSnapshot {
  return structuredClone({ nodes: api.state.canvasNodes, edges: api.state.canvasEdges });
}

test("unchanged canvas revisions avoid repeated full graph polling", async ({ page }) => {
  let revision = 0;
  let revisionRequests = 0;
  let resolveSecondRevision!: () => void;
  const secondRevision = new Promise<void>((resolve) => {
    resolveSecondRevision = resolve;
  });
  await page.route("**/v1/canvas/revision", async (route) => {
    revisionRequests += 1;
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({ revision }),
    });
    if (revisionRequests === 2) resolveSecondRevision();
  });
  let nodeRequests = 0;
  let edgeRequests = 0;
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (path === "/v1/canvas") nodeRequests += 1;
    if (path === "/v1/canvas/edges") edgeRequests += 1;
  });
  await page.goto("/");
  await secondRevision;
  expect(nodeRequests).toBe(1);
  expect(edgeRequests).toBe(1);

  const nodesRefetched = page.waitForResponse((response) =>
    new URL(response.url()).pathname === "/v1/canvas"
  );
  const edgesRefetched = page.waitForResponse((response) =>
    new URL(response.url()).pathname === "/v1/canvas/edges"
  );
  revision = 1;
  await Promise.all([nodesRefetched, edgesRefetched]);

  expect(nodeRequests).toBe(2);
  expect(edgeRequests).toBe(2);
});

function addParallelCrossProjectFixture(api: FixtureApi) {
  api.state.origins[0]!.routing_mode = "shared";
  api.state.origins[0]!.default_project_id = null;
  api.state.origins[0]!.default_project_name = null;
  api.state.projects.push({
    id: 2, name: "두 번째 프로젝트", origin_count: 0, activity_count: 1,
    needs_setup: false, latest_activity_at_us: null,
  });
  const third = structuredClone(api.state.activities[1]!);
  third.id = 3;
  third.prompt = "프로젝트 사이 연결을 확인해 주세요";
  third.project = { id: 2, name: "두 번째 프로젝트" };
  api.state.activities.push(third);
  const detail = structuredClone(api.state.details[2]!);
  detail.id = 3;
  detail.prompt = third.prompt;
  detail.project = third.project;
  api.state.details[3] = detail;
  api.state.activityOrigins[3] = 1;
  api.state.canvasNodes.push({ id: 13, activity_event_id: 3, position_x: 720, position_y: 160 });
  api.state.canvasEdges = [
    { id: 21, source_node_id: 11, target_node_id: 12 },
    { id: 22, source_node_id: 11, target_node_id: 12 },
    { id: 23, source_node_id: 11, target_node_id: 13 },
    { id: 24, source_node_id: 13, target_node_id: 11 },
  ];
  api.state.nextEdgeId = 25;
}

function canvasWrites(page: Page) {
  const writes: string[] = [];
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() !== "GET" && path.startsWith("/v1/canvas")) writes.push(`${request.method()} ${path}`);
  });
  return writes;
}

function responseFor(page: Page, method: string, path: string) {
  return page.waitForResponse((response) =>
    response.request().method() === method && new URL(response.url()).pathname === path,
  );
}

async function connect(page: Page, sourceId: number, targetId: number) {
  const created = responseFor(page, "POST", "/v1/canvas/edges");
  const source = page.getByTestId(`activity-node-${sourceId}`).locator(".react-flow__handle.source");
  const target = page.getByTestId(`activity-node-${targetId}`).locator(".react-flow__handle.target");
  const [sourceBox, targetBox] = await Promise.all([source.boundingBox(), target.boundingBox()]);
  if (!sourceBox || !targetBox) throw new Error("Custom activity handles must be measurable");
  await page.mouse.move(sourceBox.x + sourceBox.width / 2, sourceBox.y + sourceBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetBox.x + targetBox.width / 2, targetBox.y + targetBox.height / 2);
  await page.mouse.up();
  await created;
}

test("Todo 17 exposes the dashboard data coordinator without relying on an import failure", () => {
  const hook = fileURLToPath(new URL("../src/hooks/useDashboardData.ts", import.meta.url));
  const app = fileURLToPath(new URL("../src/App.tsx", import.meta.url));

  expect(existsSync(hook), "Todo 17 must provide the useDashboardData coordinator API").toBe(true);
  expect(readFileSync(app, "utf8")).toContain('from "./hooks/useDashboardData"');
});

test("one fixture session preserves durable canvas semantics through polling, filters, mutations, and deletion", async ({ page, api }) => {
  addParallelCrossProjectFixture(api);
  const writes = canvasWrites(page);
  await page.goto("/");
  await expect(page.locator(".react-flow__edge")).toHaveCount(4);

  const node = page.getByTestId("activity-node-1").locator("..");
  const box = await node.boundingBox();
  if (!box) throw new Error("Activity node must be measurable");
  const persisted = responseFor(page, "PATCH", "/v1/canvas/11");
  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 90, box.y + box.height / 2 + 70, { steps: 4 });
  await page.mouse.up();
  await persisted;
  const accepted = structuredClone(api.state.canvasNodes.find((candidate) => candidate.id === 11)!);
  const activityRefresh = page.waitForResponse((response) =>
    response.request().method() === "GET" && new URL(response.url()).pathname === "/v1/activities",
  );
  const canvasRefresh = responseFor(page, "GET", "/v1/canvas");
  const [, refreshedCanvas] = await Promise.all([activityRefresh, canvasRefresh]);
  expect(await refreshedCanvas.json()).toEqual(expect.arrayContaining([accepted]));
  const renderedPosition = await node.evaluate((element) => {
    const match = /translate\(([-\d.]+)px, ([-\d.]+)px\)/.exec((element as HTMLElement).style.transform);
    if (!match) throw new Error("React Flow node position must be rendered as a translate transform");
    return { x: Number(match[1]), y: Number(match[2]) };
  });
  expect(renderedPosition).toEqual({
    x: Number(accepted.position_x.toFixed(3)),
    y: Number(accepted.position_y.toFixed(3)),
  });

  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).toHaveCount(0);
  await expect(page.getByTestId("activity-node-3")).toHaveCount(0);
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("project:2");
  await expect(page.getByTestId("activity-node-3")).toBeVisible();
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("all");
  await expect(page.locator(".react-flow__edge")).toHaveCount(4);
  expect(api.state.canvasEdges).toEqual([
    { id: 21, source_node_id: 11, target_node_id: 12 },
    { id: 22, source_node_id: 11, target_node_id: 12 },
    { id: 23, source_node_id: 11, target_node_id: 13 },
    { id: 24, source_node_id: 13, target_node_id: 11 },
  ]);

  await connect(page, 2, 3);
  await expect(page.locator(".react-flow__edge")).toHaveCount(5);
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("all");
  await expect(page.locator(".react-flow__edge")).toHaveCount(5);
  const durableCanvas = canvas(api);
  writes.length = 0;

  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByLabel("프로젝트 이름", { exact: true }).fill("이름 변경");
  const renamed = responseFor(page, "PATCH", "/v1/projects/1");
  await page.getByRole("button", { name: "이름 저장" }).click();
  await renamed;

  await page.getByLabel("프로젝트 필터").selectOption("all");
  await page.getByTestId("activity-node-2").click();
  const assignment = page.getByRole("region", { name: "프로젝트에 배정" });
  await expect(assignment).toBeVisible();
  await assignment.getByLabel("기존 프로젝트").check();
  await assignment.getByLabel("프로젝트", { exact: true }).selectOption("1");
  const assigned = responseFor(page, "POST", "/v1/activity-assignments");
  await assignment.getByRole("button", { name: "배정 저장" }).click();
  await assigned;

  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByLabel("병합 대상").selectOption("2");
  await page.getByRole("button", { name: "병합..." }).click();
  const merged = responseFor(page, "POST", "/v1/projects/1/merge");
  await page.getByRole("button", { name: "병합 확인" }).click();
  await merged;
  expect(writes).toEqual([]);
  expect(canvas(api)).toEqual(durableCanvas);

  const activitiesBeforeDelete = structuredClone(api.state.activities);
  const toggled = responseFor(page, "PATCH", "/v1/providers/codex");
  await page.getByRole("checkbox", { name: "Codex capture" }).uncheck();
  await toggled;
  expect(api.state.provider.enabled).toBe(false);
  expect(api.state.activities).toEqual(activitiesBeforeDelete);

  await page.getByLabel("프로젝트 필터").selectOption("all");
  await page.getByTestId("activity-node-1").click();
  const deleted = responseFor(page, "DELETE", "/v1/canvas/11");
  await page.keyboard.press("Backspace");
  await deleted;
  expect(api.state.activities).toEqual(activitiesBeforeDelete);
  const cleared = responseFor(page, "DELETE", "/v1/canvas");
  await page.getByRole("button", { name: "Clear canvas" }).click();
  await cleared;
  expect(api.state.canvasNodes).toEqual([]);
  expect(api.state.canvasEdges).toEqual([]);
  expect(api.state.activities).toEqual(activitiesBeforeDelete);
});
