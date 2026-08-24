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

test("empty dashboards guide the first capture and keep destructive actions out of the way", async ({
  page,
  api,
}) => {
  api.state.activities = [];
  api.state.canvasNodes = [];
  api.state.canvasEdges = [];
  api.state.origins = [];

  await page.goto("/");

  await expect(page.getByText("No activity on this canvas")).toBeVisible();
  await expect(page.getByText("Enable Codex capture and submit a prompt to add your first activity.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Clear canvas" })).toHaveCount(0);
  await expect(page.getByText("No work locations yet")).toBeVisible();
});

test("a period changes nodes and navigation counts together, then can hide empty projects", async ({
  page,
  api,
}) => {
  const now = Date.now();
  const localNow = new Date(now);
  const localTodayStart = new Date(
    localNow.getFullYear(),
    localNow.getMonth(),
    localNow.getDate(),
  ).getTime();
  for (const activity of api.state.activities) {
    activity.time = { value: new Date(now).toISOString(), provenance: "captured" };
  }
  const lateYesterday = structuredClone(api.state.activities[0]!);
  lateYesterday.id = 4;
  lateYesterday.prompt = "자정 직전의 24시간 내 활동";
  lateYesterday.time = {
    value: new Date(localTodayStart - 1).toISOString(),
    provenance: "captured",
  };
  api.state.activities.push(lateYesterday);
  api.state.activityOrigins[4] = 1;
  api.state.canvasNodes.push({ id: 14, activity_event_id: 4, position_x: 560, position_y: 360 });
  const old = structuredClone(api.state.activities[0]!);
  old.id = 3;
  old.prompt = "오래된 프로젝트 활동";
  old.project = { id: 2, name: "보관 프로젝트" };
  old.time = {
    value: new Date(now - 91 * 24 * 60 * 60 * 1_000).toISOString(),
    provenance: "captured",
  };
  api.state.activities.push(old);
  const oldDetail = structuredClone(api.state.details[1]!);
  oldDetail.id = 3;
  oldDetail.prompt = old.prompt;
  oldDetail.project = old.project;
  oldDetail.captured_at = old.time;
  oldDetail.first_recorded_at = old.time;
  api.state.details[3] = oldDetail;
  api.state.projects.push({
    id: 2,
    name: "보관 프로젝트",
    origin_count: 0,
    activity_count: 1,
    needs_setup: false,
    latest_activity_at_us: null,
  });
  api.state.activityOrigins[3] = 1;
  api.state.canvasNodes.push({ id: 13, activity_event_id: 3, position_x: 720, position_y: 160 });

  await page.goto("/");
  await page.getByLabel("기간 필터").selectOption("today");
  await expect(page.getByTestId("activity-node-4")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /All activity/ })).toContainText("2");
  await expect(page.getByRole("button", { name: /^기존 프로젝트 \d+$/ })).toContainText("1");

  await page.getByLabel("기간 필터").selectOption("day");
  await expect(page.getByTestId("activity-node-4")).toBeVisible();
  await expect(page.getByRole("button", { name: /All activity/ })).toContainText("3");
  await expect(page.getByRole("button", { name: /^기존 프로젝트 \d+$/ })).toContainText("2");

  await page.getByLabel("기간 필터").selectOption("week");

  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByTestId("activity-node-3")).toHaveCount(0);
  await expect(page.getByRole("button", { name: /All activity/ })).toContainText("3");
  await expect(page.getByRole("button", { name: /보관 프로젝트/ })).toContainText("0");

  await page.getByLabel("프로젝트 필터").selectOption("project:2");
  await expect(page.getByTestId("activity-node-1")).toHaveCount(0);
  const hideEmpty = page.getByRole("checkbox", { name: "결과 없는 프로젝트 숨기기" });
  await hideEmpty.check();

  await expect(page.getByLabel("프로젝트 필터")).toHaveValue("all");
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByRole("button", { name: /보관 프로젝트/ })).toHaveCount(0);
  await expect(page.locator('option[value="project:2"]')).toHaveCount(0);
});

test("the canvas header stays readable on narrow screens and supports wheel zoom", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  const heading = page.getByRole("heading", { name: "Prompt canvas" });
  const clear = page.getByRole("button", { name: "Clear canvas" });
  const [headingBox, clearBox] = await Promise.all([heading.boundingBox(), clear.boundingBox()]);
  if (!headingBox || !clearBox) throw new Error("Canvas header controls must be measurable");
  expect(headingBox.x + headingBox.width).toBeLessThanOrEqual(clearBox.x + 1);
  expect(clearBox.height).toBeLessThanOrEqual(42);

  const viewport = page.locator(".react-flow__viewport");
  const scale = async () => viewport.evaluate((element) => {
    const transform = getComputedStyle(element).transform;
    const match = /^matrix\(([-\d.]+)/.exec(transform);
    if (!match) throw new Error(`Unexpected viewport transform: ${transform}`);
    return Number(match[1]);
  });
  const initialScale = await scale();
  const flow = page.locator(".react-flow");
  await flow.scrollIntoViewIfNeeded();
  const flowBox = await flow.boundingBox();
  if (!flowBox) throw new Error("Canvas must be measurable");
  await page.mouse.move(flowBox.x + flowBox.width / 2, flowBox.y + flowBox.height / 2);
  await page.mouse.wheel(0, -360);
  await expect.poll(scale).toBeGreaterThan(initialScale);
});

test("detected Codex installations expose independent hook controls", async ({ page, api }) => {
  await page.goto("/");

  await expect(page.getByText("2개 중 1개 hook 설치")).toBeVisible();
  await expect(page.getByText("C:\\Users\\fixture\\.codex\\hooks.json")).toBeVisible();
  await expect(page.getByText("/home/fixture/.codex/hooks.json")).toBeVisible();
  await expect(page.getByText(/Akra가 현재 hook 정의만 자동으로 신뢰/)).toBeVisible();
  await expect(page.getByText("/hooks", { exact: true })).toHaveCount(0);

  const wslCapture = page.getByRole("checkbox", { name: "Codex · Ubuntu capture" });
  const wslEnabled = responseFor(
    page,
    "PATCH",
    "/v1/providers/codex/targets/wsl%3AUbuntu",
  );
  await wslCapture.click();
  await wslEnabled;
  await expect(page.getByText("2개 중 2개 hook 설치")).toBeVisible();
  expect(api.state.provider.targets.find(({ id }) => id === "wsl:Ubuntu")?.enabled).toBe(true);

  const windowsCapture = page.getByRole("checkbox", { name: "Codex App + CLI capture" });
  const windowsDisabled = responseFor(
    page,
    "PATCH",
    "/v1/providers/codex/targets/windows-native",
  );
  await windowsCapture.click();
  await windowsDisabled;
  await expect(page.getByText("2개 중 1개 hook 설치")).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Codex capture" })).toBeChecked();

  const wslDisabled = responseFor(
    page,
    "PATCH",
    "/v1/providers/codex/targets/wsl%3AUbuntu",
  );
  await wslCapture.click();
  await wslDisabled;
  await expect(page.getByText("2개 중 0개 hook 설치")).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Codex capture" })).not.toBeChecked();
});

test("smart prompt summaries are an explicit capture setting and do not touch hook targets", async ({
  page,
  api,
}) => {
  await page.goto("/");
  const toggle = page.getByRole("checkbox", { name: "문맥 기반 프롬프트 요약" });
  await expect(toggle).not.toBeChecked();
  await expect(page.getByText("Off · 제출한 원문을 그대로 표시", { exact: true })).toBeVisible();

  const changed = responseFor(page, "PUT", "/v1/providers/codex/prompt-summaries");
  await toggle.check();
  const request = await changed;
  expect(request.request().postDataJSON()).toEqual({ mode: "smart" });
  expect(api.state.provider.prompt_summary_mode).toBe("smart");
  await expect(page.getByText("Smart · 앞선 결과 요약만 문맥으로 사용", { exact: true })).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Codex App + CLI capture" })).toBeChecked();
  await expect(page.getByRole("checkbox", { name: "Codex · Ubuntu capture" })).not.toBeChecked();
});

test("one shared Windows hook reports App and CLI capture evidence independently", async ({ page, api }) => {
  const windows = api.state.provider.targets.find(({ id }) => id === "windows-native");
  if (!windows) throw new Error("Windows Codex fixture target is required");
  windows.activation = "verified";
  windows.clients = [
    {
      id: "app",
      label: "Codex App",
      verified: false,
      last_captured_at_us: null,
    },
    {
      id: "cli",
      label: "Codex CLI",
      verified: true,
      last_captured_at_us: 1_786_176_000_000_000,
    },
  ];

  await page.goto("/");

  const target = page.getByRole("listitem").filter({ hasText: "Codex App + CLI" });
  await expect(target.getByText("Capture verified")).toBeVisible();
  await expect(target.getByText("Codex App", { exact: true })).toBeVisible();
  await expect(target.getByText("설치 후 캡처 없음", { exact: true })).toBeVisible();
  await expect(target.getByText("Codex CLI", { exact: true })).toBeVisible();
  await expect(target.getByText(/캡처 확인/)).toBeVisible();
  await expect(page.getByRole("checkbox", { name: "Codex App + CLI capture" })).toHaveCount(1);
  await expect(page.getByRole("checkbox", { name: "Codex App capture" })).toHaveCount(0);
  await expect(page.getByRole("checkbox", { name: "Codex CLI capture" })).toHaveCount(0);
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
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  await deleted;
  const activitiesAfterDelete = activitiesBeforeDelete
    .filter(({ id }) => id !== 1)
    .map((activity, index, remaining) => ({
      ...activity,
      conversation_index: index + 1,
      conversation_total: remaining.length,
    }));
  expect(api.state.activities).toEqual(activitiesAfterDelete);
  const clearButton = page.getByRole("button", { name: "Clear canvas" });
  await clearButton.click();
  const clearDialog = page.getByRole("dialog", { name: "Canvas를 비울까요?" });
  await expect(clearDialog).toContainText("저장된 prompt history는 유지됩니다.");
  await clearDialog.getByRole("button", { name: "취소" }).click();
  await expect(clearDialog).toHaveCount(0);
  await expect(clearButton).toBeFocused();
  expect(api.state.canvasNodes.length).toBeGreaterThan(0);
  const cleared = responseFor(page, "DELETE", "/v1/canvas");
  await clearButton.click();
  await page.getByRole("button", { name: "Canvas 비우기" }).click();
  await cleared;
  await expect(page.locator(".flow-stage")).toBeFocused();
  expect(api.state.canvasNodes).toEqual([]);
  expect(api.state.canvasEdges).toEqual([]);
  expect(api.state.activities).toEqual(activitiesAfterDelete);
});
