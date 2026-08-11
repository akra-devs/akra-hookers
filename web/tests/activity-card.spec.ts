import { expect, test as base } from "@playwright/test";

import { FixtureApi, fixtureApiUrl, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [
    async ({ page }, use) => {
      await use(await installFixtureApi(page));
    },
    { auto: true },
  ],
});

test.use({ locale: "ko-KR", timezoneId: "Asia/Seoul" });

test("activity cards show context hierarchy and truthful time states", async ({ page, api }) => {
  api.state.activities[1]!.time = {
    value: "2026-08-08T12:00:00Z",
    provenance: "legacy_recorded",
  };
  api.state.activities.push({
    id: 3,
    provider: "codex",
    prompt: "시간 정보가 없는 작업",
    project: null,
    time: { value: null, provenance: "unknown" },
    conversation_index: 1,
    conversation_total: 1,
  });
  api.state.canvasNodes.push({
    id: 13,
    activity_event_id: 3,
    position_x: 760,
    position_y: 160,
  });

  await page.goto("/");

  const projectCard = page.getByTestId("activity-node-1");
  await expect(projectCard.getByText("기존 프로젝트")).toBeVisible();
  await expect(projectCard.getByText("프로젝트 이름을 정리해 주세요")).toBeVisible();
  await expect(projectCard.getByText("codex")).toBeVisible();
  await expect(projectCard.getByText("2026. 8. 8. 오후 9:00")).toBeVisible();
  await expect(projectCard.getByText("1/2", { exact: true })).toBeVisible();
  await expect(projectCard.locator(".activity-node__prompt")).toHaveCSS("-webkit-line-clamp", "3");

  await expect(page.getByTestId("activity-node-2").getByText("분류 필요")).toBeVisible();
  await expect(page.getByTestId("activity-node-2").getByText(/기존 기록/)).toBeVisible();
  await expect(page.getByTestId("activity-node-3").getByText("시간 정보 없음")).toBeVisible();
  await page.screenshot({
    path: "../.omo/evidence/task-13-project-context-and-conversation.png",
    fullPage: true,
  });
});

test("activity card DOM and accessibility omit detail-only secrets", async ({ page, api }) => {
  const detail = api.state.details[1]!;
  detail.technical.session_id = "SESSION_SECRET_57";
  detail.technical.turn_id = "TURN_SECRET_91";
  detail.submitted_cwd = "C:\\SECRET\\WORKTREE\\PATH";

  await page.goto("/");

  const body = page.locator("body");
  const html = await body.evaluate((element) => element.outerHTML);
  const accessibility = await body.ariaSnapshot();
  for (const secret of ["SESSION_SECRET_57", "TURN_SECRET_91", "SECRET\\WORKTREE"]) {
    expect(html).not.toContain(secret);
    expect(accessibility).not.toContain(secret);
  }
  const explicitDetail = await api.dispatch(
    "GET",
    new URL(`${fixtureApiUrl}/v1/activities/1`),
    { authorization: "Bearer fixture-token" },
    undefined,
  );
  expect(explicitDetail.body).toEqual(expect.objectContaining({
    technical: {
      session_id: "SESSION_SECRET_57",
      turn_id: "TURN_SECRET_91",
    },
    submitted_cwd: "C:\\SECRET\\WORKTREE\\PATH",
  }));
});

test("two custom card handles persist exactly one edge", async ({ page, api }) => {
  api.state.canvasEdges = [];
  await page.goto("/");
  const request = page.waitForRequest((candidate) =>
    candidate.method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/canvas/edges");
  const source = page.getByTestId("activity-node-1").locator(".react-flow__handle.source");
  const target = page.getByTestId("activity-node-2").locator(".react-flow__handle.target");
  const sourceBox = await source.boundingBox();
  const targetBox = await target.boundingBox();
  expect(sourceBox).not.toBeNull();
  expect(targetBox).not.toBeNull();
  if (!sourceBox || !targetBox) throw new Error("Activity handles must be measurable");

  await page.mouse.move(sourceBox.x + sourceBox.width / 2, sourceBox.y + sourceBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetBox.x + targetBox.width / 2, targetBox.y + targetBox.height / 2);
  await page.mouse.up();
  await request;

  expect(api.state.canvasEdges).toHaveLength(1);
  await expect(page.locator(".react-flow__edge")).toHaveCount(1);
});

test("selecting a custom card creates no edge", async ({ page, api }) => {
  api.state.canvasEdges = [];
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  await card.click();

  await expect(card.locator("..")).toHaveClass(/selected/);
  expect(api.state.canvasEdges).toHaveLength(0);
  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
});

test("dragging a custom card persists its changed canvas position", async ({ page, api }) => {
  await page.goto("/");
  const card = page.getByTestId("activity-node-1");
  const node = card.locator("..");
  const box = await node.boundingBox();
  expect(box).not.toBeNull();
  if (!box) throw new Error("Activity card must be measurable");
  const before = { ...api.state.canvasNodes[0]! };
  const request = page.waitForRequest((candidate) =>
    candidate.method() === "PATCH"
    && new URL(candidate.url()).pathname === "/v1/canvas/11");

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 80, box.y + box.height / 2 + 60, {
    steps: 5,
  });
  await page.mouse.up();
  const persisted = await request;

  expect(persisted.postDataJSON()).not.toEqual({
    position_x: before.position_x,
    position_y: before.position_y,
  });
  expect(api.state.canvasNodes[0]).not.toMatchObject({
    position_x: before.position_x,
    position_y: before.position_y,
  });
  expect(api.state.canvasEdges).toHaveLength(1);
});

test("dragging a selected group persists every changed card position", async ({ page, api }) => {
  await page.goto("/");
  const firstCard = page.getByTestId("activity-node-1");
  const secondCard = page.getByTestId("activity-node-2");
  await firstCard.click();
  await page.keyboard.down("Control");
  await secondCard.click();
  await page.keyboard.up("Control");
  const firstNode = firstCard.locator("..");
  const box = await firstNode.boundingBox();
  expect(box).not.toBeNull();
  if (!box) throw new Error("Selected activity card must be measurable");
  const before = structuredClone(api.state.canvasNodes);
  const firstPatch = page.waitForRequest((candidate) =>
    candidate.method() === "PATCH"
    && new URL(candidate.url()).pathname === "/v1/canvas/11");
  const secondPatch = page.waitForRequest((candidate) =>
    candidate.method() === "PATCH"
    && new URL(candidate.url()).pathname === "/v1/canvas/12");

  await page.mouse.move(box.x + box.width / 2, box.y + box.height / 2);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width / 2 + 90, box.y + box.height / 2 + 50, {
    steps: 5,
  });
  await page.mouse.up();
  const [firstRequest, secondRequest] = await Promise.all([firstPatch, secondPatch]);

  expect(firstRequest.postDataJSON()).toEqual({
    position_x: api.state.canvasNodes[0]!.position_x,
    position_y: api.state.canvasNodes[0]!.position_y,
  });
  expect(secondRequest.postDataJSON()).toEqual({
    position_x: api.state.canvasNodes[1]!.position_x,
    position_y: api.state.canvasNodes[1]!.position_y,
  });
  expect(api.state.canvasNodes[0]).not.toMatchObject({
    position_x: before[0]!.position_x,
    position_y: before[0]!.position_y,
  });
  expect(api.state.canvasNodes[1]).not.toMatchObject({
    position_x: before[1]!.position_x,
    position_y: before[1]!.position_y,
  });
  await page.reload();
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).toBeVisible();
});

test("keyboard delete removes only the selected canvas edge", async ({ page, api }) => {
  await page.goto("/");
  const edge = page.locator(".react-flow__edge").first();
  await edge.locator(".react-flow__edge-interaction").click();
  await expect(edge).toHaveClass(/selected/);

  const deleted = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/canvas/edges/21",
  );
  await page.keyboard.press("Backspace");
  expect((await deleted).status()).toBe(204);

  await expect(page.locator(".react-flow__edge")).toHaveCount(0);
  expect(api.state.canvasEdges).toEqual([]);
  expect(api.state.canvasNodes).toHaveLength(2);
  expect(api.state.activities).toHaveLength(2);
});

test("keyboard delete removes only the custom canvas node", async ({ page, api }) => {
  await page.goto("/");
  const card = page.getByTestId("activity-node-1");
  await card.click();
  await expect(card.locator("..")).toHaveClass(/selected/);
  const deleted = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/canvas/11", { timeout: 1_000 })
    .catch(() => null);

  await page.keyboard.press("Delete");
  const response = await deleted;

  expect({
    deleteStatus: response?.status() ?? null,
    canvasNodePresent: api.state.canvasNodes.some((node) => node.activity_event_id === 1),
    activityPresent: api.state.activities.some((activity) => activity.id === 1),
  }).toEqual({
    deleteStatus: 204,
    canvasNodePresent: false,
    activityPresent: true,
  });

  await expect(card).toHaveCount(0);
});
