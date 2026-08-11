import { expect, test as base, type Page } from "@playwright/test";

import type { ActivityAssignmentRequest } from "../src/api";
import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [
    async ({ page }, use) => {
      await use(await installFixtureApi(page));
    },
    { auto: true },
  ],
});

test.use({ locale: "ko-KR", timezoneId: "Asia/Seoul" });

function sharedInbox(api: FixtureApi) {
  const origin = api.state.origins[0]!;
  origin.routing_mode = "shared";
  origin.default_project_id = null;
  origin.default_project_name = null;
  for (const activity of api.state.activities) activity.project = null;
  for (const detail of Object.values(api.state.details)) {
    detail.project = null;
    for (const turn of detail.conversation) turn.project = null;
  }
}

function canvas(api: FixtureApi) {
  return structuredClone({ nodes: api.state.canvasNodes, edges: api.state.canvasEdges });
}

function dedicatedCard(api: FixtureApi) {
  const activity = structuredClone(api.state.activities[0]!);
  activity.id = 3;
  activity.prompt = "전용 위치 활동";
  activity.project = { id: 1, name: "기존 프로젝트" };
  api.state.activities.push(activity);
  const detail = structuredClone(api.state.details[1]!);
  detail.id = 3;
  detail.prompt = activity.prompt;
  detail.project = activity.project;
  detail.origin.id = api.state.origins[1]!.id;
  detail.origin.display_path = api.state.origins[1]!.display_path;
  detail.origin.kind = api.state.origins[1]!.kind;
  api.state.details[3] = detail;
  api.state.activityOrigins[3] = 2;
  api.state.origins[1]!.setup_state = "confirmed";
  api.state.origins[1]!.routing_mode = "dedicated";
  api.state.origins[1]!.default_project_id = 1;
  api.state.origins[1]!.default_project_name = "기존 프로젝트";
  api.state.canvasNodes.push({ id: 13, activity_event_id: 3, position_x: 720, position_y: 160 });
}

async function select(page: Page, ...ids: number[]) {
  const [first, ...rest] = ids;
  if (first === undefined) throw new Error("A selection needs an activity");
  await page.getByTestId(`activity-node-${first}`).click();
  if (rest.length > 0) await page.keyboard.down("Control");
  for (const id of rest) await page.getByTestId(`activity-node-${id}`).click();
  if (rest.length > 0) await page.keyboard.up("Control");
}

function assignmentBar(page: Page) {
  return page.getByRole("region", { name: "프로젝트에 배정" });
}

async function submitAssignment(
  page: Page,
  expected: ActivityAssignmentRequest,
) {
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/activity-assignments"
    && candidate.status() === 200);
  await assignmentBar(page).getByRole("button", { name: "배정 저장" }).click();
  expect((await response).request().postDataJSON()).toEqual(expected);
}

test("Inbox multi-selection assigns only selected cards and restores an unassigned card", async ({ page, api }) => {
  sharedInbox(api);
  const before = canvas(api);
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  await select(page, 1, 2);
  const canvasBottom = await page.locator(".react-flow").evaluate((element) => element.getBoundingClientRect().bottom);
  const contextBottom = await page.getByTestId("activity-node-2").locator(".activity-node__meta").evaluate((element) => element.getBoundingClientRect().bottom);
  expect(contextBottom).toBeLessThanOrEqual(canvasBottom);
  await page.screenshot({
    path: "../.omo/evidence/task-15-project-context-and-conversation.png",
    fullPage: true,
  });

  const bar = assignmentBar(page);
  const futureRoute = bar.getByLabel("이 대화의 이후 활동도 이 프로젝트에 배정");
  await expect(futureRoute).not.toBeChecked();
  await bar.getByLabel("기존 프로젝트").check();
  await bar.getByLabel("프로젝트", { exact: true }).selectOption("1");
  await submitAssignment(page, {
    activity_ids: [1, 2],
    destination: { project_id: 1 },
    future_route: "unchanged",
  });

  await expect(page.getByTestId("activity-node-1")).toHaveCount(0);
  await expect(page.getByTestId("activity-node-2")).toHaveCount(0);
  expect(canvas(api)).toEqual(before);
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).toBeVisible();

  await select(page, 2);
  await assignmentBar(page).getByLabel("분류 필요").check();
  await expect(assignmentBar(page).getByLabel(
    "이 대화의 이후 활동도 이 프로젝트에 배정",
  )).toHaveCount(0);
  await submitAssignment(page, {
    activity_ids: [2],
    destination: null,
    future_route: "unchanged",
  });
  await expect(page.getByTestId("activity-node-2")).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  await expect(page.getByTestId("activity-node-2")).toBeVisible();
  expect(canvas(api)).toEqual(before);
});

test("assigning a loaded historical Inbox card invalidates its cached summary", async ({
  page,
  api,
}) => {
  sharedInbox(api);
  const seed = api.state.activities[1]!;
  for (let id = 3; id <= 101; id += 1) {
    api.state.activities.push({
      ...structuredClone(seed),
      id,
      prompt: `historical inbox prompt ${id}`,
      conversation_index: id,
      conversation_total: 101,
    });
    api.state.canvasNodes.push({
      id: 1000 + id,
      activity_event_id: id,
      position_x: id * 10,
      position_y: id * 7,
    });
  }
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  await page.getByRole("button", { name: "이전 활동 불러오기" }).click();
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await select(page, 1);
  const bar = assignmentBar(page);
  await bar.getByLabel("기존 프로젝트").check();
  await bar.getByLabel("프로젝트", { exact: true }).selectOption("1");

  await submitAssignment(page, {
    activity_ids: [1],
    destination: { project_id: 1 },
    future_route: "unchanged",
  });

  await expect(page.getByTestId("activity-node-1")).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
});

test("opening the assignment bar under an active pointer never persists node movement", async ({ page, api }) => {
  sharedInbox(api);
  const before = canvas(api);
  const moveRequests: string[] = [];
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (request.method() === "PATCH" && /^\/v1\/canvas\/\d+$/.test(path)) {
      moveRequests.push(path);
    }
  });
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  const node = page.getByTestId("activity-node-1");
  const bounds = await node.boundingBox();
  if (!bounds) throw new Error("activity node needs visible bounds");

  const pointer = {
    x: bounds.x + bounds.width / 2,
    y: bounds.y + bounds.height / 2,
  };
  await page.mouse.move(pointer.x, pointer.y);
  await page.mouse.down();
  await page.mouse.move(pointer.x + 2, pointer.y);
  await expect(assignmentBar(page)).toBeVisible();
  await page.mouse.move(pointer.x + 2, pointer.y);
  await page.mouse.up();
  const destination = assignmentBar(page).getByLabel("기존 프로젝트");
  await destination.check();
  await expect(destination).toBeChecked();

  expect(moveRequests).toEqual([]);
  expect(canvas(api)).toEqual(before);
});

test("a shared conversation sets, replaces, and clears only its future route", async ({ page, api }) => {
  sharedInbox(api);
  api.state.projects[0]!.name = "A";
  const before = canvas(api);
  await page.goto("/");
  await select(page, 1);

  let bar = assignmentBar(page);
  await bar.getByLabel("기존 프로젝트").check();
  await bar.getByLabel("프로젝트", { exact: true }).selectOption("1");
  await bar.getByLabel("이 대화의 이후 활동도 이 프로젝트에 배정").check();
  await submitAssignment(page, {
    activity_ids: [1], destination: { project_id: 1 }, future_route: "set",
  });
  expect(api.state.activities[0]!.project).toEqual({ id: 1, name: "A" });
  expect(api.state.activities[1]!.project).toBeNull();
  expect(api.state.conversationRoutes).toEqual({ "codex:fixture-session": 1 });

  await page.reload();
  await select(page, 2);
  bar = assignmentBar(page);
  const newProjectId = api.state.nextProjectId;
  await bar.getByLabel("새 프로젝트").check();
  await bar.getByLabel("새 프로젝트 이름").fill("B");
  await bar.getByLabel("이 대화의 이후 활동도 이 프로젝트에 배정").check();
  await submitAssignment(page, {
    activity_ids: [2], destination: { new_project_name: "B" }, future_route: "set",
  });
  expect(api.state.activities.map((activity) => activity.project?.name)).toEqual(["A", "B"]);
  expect(api.state.conversationRoutes).toEqual({
    "codex:fixture-session": newProjectId,
  });

  await page.reload();
  await select(page, 1, 2);
  await expect(assignmentBar(page).getByRole(
    "button",
    { name: "이후 활동 배정 해제" },
  )).toHaveCount(0);

  await page.reload();
  await select(page, 2);
  const clear = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/activity-assignments"
    && candidate.status() === 200);
  await assignmentBar(page).getByRole("button", { name: "이후 활동 배정 해제" }).click();
  expect((await clear).request().postDataJSON()).toEqual({
    activity_ids: [2],
    destination: { project_id: newProjectId },
    future_route: "clear",
  });
  expect(api.state.conversationRoutes).toEqual({});
  expect(api.state.activities.map((activity) => activity.project?.name)).toEqual(["A", "B"]);
  expect(canvas(api)).toEqual(before);
});

test("mixed sessions suppress routing while dedicated cards expose only work-location movement", async ({ page, api }) => {
  sharedInbox(api);
  dedicatedCard(api);
  api.state.details[2]!.technical.session_id = "different-session";
  await page.goto("/");
  await select(page, 1, 2);
  await expect(assignmentBar(page)).toBeVisible();
  await expect(assignmentBar(page).getByLabel(
    "이 대화의 이후 활동도 이 프로젝트에 배정",
  )).toHaveCount(0);

  await page.reload();
  await select(page, 3);
  await expect(page.getByRole("button", { name: "작업 위치 이동" })).toBeVisible();
  await expect(page.getByRole("region", { name: "프로젝트에 배정" })).toHaveCount(0);
  await page.screenshot({
    path: "../.omo/evidence/task-15-dedicated-guardrail.png",
    fullPage: true,
  });
});

test("a 422 assignment preserves selection, route intent, cards, and canvas state", async ({ page, api }) => {
  sharedInbox(api);
  const before = structuredClone(api.state);
  const beforeCanvas = canvas(api);
  await page.route("**/v1/activity-assignments", async (route) => {
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "stale_or_mixed", message: "선택을 함께 배정할 수 없습니다." }),
    });
  });
  await page.goto("/");
  await select(page, 1, 2);
  const bar = assignmentBar(page);
  const futureRoute = bar.getByLabel("이 대화의 이후 활동도 이 프로젝트에 배정");
  await bar.getByLabel("기존 프로젝트").check();
  await bar.getByLabel("프로젝트", { exact: true }).selectOption("1");
  await futureRoute.check();
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/activity-assignments"
    && candidate.status() === 422);
  await bar.getByRole("button", { name: "배정 저장" }).click();
  await response;

  await expect(page.getByRole("alert")).toHaveText("선택을 함께 배정할 수 없습니다.");
  await expect(futureRoute).toBeChecked();
  await expect(page.getByTestId("activity-node-1").locator("..")).toHaveClass(/selected/);
  await expect(page.getByTestId("activity-node-2").locator("..")).toHaveClass(/selected/);
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).toBeVisible();
  await page.screenshot({
    path: "../.omo/evidence/task-15-inline-error.png",
    fullPage: true,
  });
  expect(api.state).toEqual(before);
  expect(canvas(api)).toEqual(beforeCanvas);
});

test("the Korean assignment bar fits a narrow viewport", async ({ page, api }) => {
  sharedInbox(api);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  await select(page, 1, 2);
  await expect(assignmentBar(page).getByRole("button", { name: "배정 저장" })).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth <= innerWidth)).toBe(true);
  await page.screenshot({ path: "../.omo/evidence/task-15-narrow-assignment.png", fullPage: true });
});
