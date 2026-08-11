import { expect, test as base, type Page } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [async ({ page }, use) => use(await installFixtureApi(page)), { auto: true }],
});

test.use({ locale: "ko-KR", timezoneId: "Asia/Seoul" });

function durableCanvas(api: FixtureApi): string {
  return JSON.stringify({ nodes: api.state.canvasNodes, edges: api.state.canvasEdges });
}

function canvasEndpoints(page: Page): string[] {
  const calls: string[] = [];
  page.on("request", (request) => {
    const path = new URL(request.url()).pathname;
    if (
      (request.method() === "POST" && path === "/v1/canvas/edges")
      || (request.method() === "PATCH" && /^\/v1\/canvas\/\d+$/.test(path))
    ) {
      calls.push(`${request.method()} ${path}`);
    }
  });
  return calls;
}

async function dragActivity(page: Page, activityId: number) {
  const node = page.getByTestId(`activity-node-${activityId}`).locator("..");
  const box = await node.boundingBox();
  expect(box).not.toBeNull();
  if (!box) throw new Error("Activity card must be measurable");

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 60, {
    steps: 5,
  });
  await page.mouse.up();
}

test("bootstrap query failures replace false empty state and retry explicitly", async ({ page }) => {
  let rejectBootstrap = true;
  await page.route("**/v1/activities**", async (route) => {
    const scope = new URL(route.request().url()).searchParams.get("scope");
    if (!rejectBootstrap || scope !== "all") {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ code: "activities_unavailable", message: "활동 조회 실패" }),
    });
  });
  await page.route("**/v1/providers/codex", async (route) => {
    if (!rejectBootstrap || route.request().method() !== "GET") {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({ code: "provider_unavailable", message: "Provider 조회 실패" }),
    });
  });
  await page.goto("/");

  await expect(page.getByRole("alert")).toContainText(
    "대시보드 데이터를 불러오지 못했습니다",
  );
  await expect(page.getByText("No activity on this canvas")).toHaveCount(0);
  await expect(page.getByLabel("Codex capture")).toBeDisabled();

  rejectBootstrap = false;
  await page.getByRole("button", { name: "다시 시도" }).click();
  await expect(page.getByRole("alert")).toHaveCount(0);
  await expect(page.getByLabel("Codex capture")).toBeEnabled();
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
});

test("a rejected position mutation refetches the exact server canvas without duplicating immutable history", async ({ page, api }) => {
  const beforeCanvas = durableCanvas(api);
  const beforeActivities = JSON.stringify(api.state.activities);
  const serverPosition = { ...api.state.canvasNodes[0]! };
  let rejected = false;
  const canvasMutation = page.waitForRequest((request) =>
    request.method() === "PATCH" && new URL(request.url()).pathname === "/v1/canvas/11",
  );
  const refetchedCanvas = page.waitForResponse((response) =>
    rejected
    && response.request().method() === "GET"
    && new URL(response.url()).pathname === "/v1/canvas"
    && response.status() === 200,
  );
  await page.route("**/v1/canvas/11", async (route) => {
    if (route.request().method() !== "PATCH") return route.fallback();
    rejected = true;
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "stale_position", message: "위치가 최신 상태가 아닙니다." }),
    });
  });

  await page.goto("/");
  const node = page.getByTestId("activity-node-1").locator("..");
  const serverTransform = await node.evaluate((element) => getComputedStyle(element).transform);
  await dragActivity(page, 1);
  expect((await canvasMutation).postDataJSON()).not.toEqual({
    position_x: serverPosition.position_x,
    position_y: serverPosition.position_y,
  });
  await refetchedCanvas;

  await expect(node).toHaveCSS("transform", serverTransform);
  await expect(page.getByTestId(/activity-node-/)).toHaveCount(2);
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  await expect(page.getByTestId("activity-node-1")).toContainText("프로젝트 이름을 정리해 주세요");
  expect(durableCanvas(api)).toBe(beforeCanvas);
  expect(JSON.stringify(api.state.activities)).toBe(beforeActivities);
});

test("a rejected project rename never mutates durable canvas state", async ({ page, api }) => {
  const beforeCanvas = durableCanvas(api);
  const canvasCalls = canvasEndpoints(page);
  await page.route("**/v1/projects/1", async (route) => {
    if (route.request().method() !== "PATCH") return route.fallback();
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "invalid_name", message: "프로젝트 이름을 바꿀 수 없습니다." }),
    });
  });

  await page.goto("/");
  const rename = page.waitForResponse((response) =>
    response.request().method() === "PATCH"
    && new URL(response.url()).pathname === "/v1/projects/1"
    && response.status() === 422,
  );
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByLabel("프로젝트 이름", { exact: true }).fill("거부된 이름");
  await page.getByRole("button", { name: "이름 저장" }).click();
  await rename;

  await expect(page.getByRole("alert")).toHaveText("프로젝트 이름을 바꿀 수 없습니다.");
  await expect(page.getByLabel("프로젝트 이름", { exact: true })).toHaveValue("거부된 이름");
  await page.getByLabel("프로젝트 필터").selectOption("all");
  await expect(page.getByTestId(/activity-node-/)).toHaveCount(2);
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
  expect(canvasCalls).toEqual([]);
  expect(durableCanvas(api)).toBe(beforeCanvas);
});
