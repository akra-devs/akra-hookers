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
    activity_kind: "user",
    prompt: "시간 정보가 없는 작업",
    project: null,
    time: { value: null, provenance: "unknown" },
    previous_conversation_activity_id: null,
    conversation_index: 1,
    conversation_total: 1,
    result_summary_status: "unavailable",
    prompt_summary: { status: "unavailable", mode: "fallback", text: null },
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
  await expect(page.getByTestId("activity-node-2").getByText("2026. 8. 8. 오후 9:00")).toBeVisible();
  await expect(page.getByTestId("activity-node-2")).not.toContainText("기존 기록");
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
    technical: expect.objectContaining({
      session_id: "SESSION_SECRET_57",
      turn_id: "TURN_SECRET_91",
    }),
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

test("conversation requests receive non-editable directional edges in order", async ({ page, api }) => {
  api.state.canvasEdges = [];
  api.state.activities[0]!.previous_conversation_activity_id = null;
  api.state.activities[0]!.conversation_total = 3;
  api.state.activities[1]!.previous_conversation_activity_id = 1;
  api.state.activities[1]!.conversation_total = 3;
  api.state.activities.push({
    ...structuredClone(api.state.activities[1]!),
    id: 3,
    prompt: "세 번째 요청",
    previous_conversation_activity_id: 2,
    conversation_index: 3,
  });
  api.state.canvasNodes.push({
    id: 13,
    activity_event_id: 3,
    position_x: 420,
    position_y: 440,
  });
  let edgeMutationRequests = 0;
  page.on("request", (request) => {
    const pathname = new URL(request.url()).pathname;
    if (pathname === "/v1/canvas/edges" && request.method() === "POST") {
      edgeMutationRequests += 1;
    }
    if (pathname.startsWith("/v1/canvas/edges/") && request.method() === "DELETE") {
      edgeMutationRequests += 1;
    }
  });

  await page.goto("/");

  const firstToSecond = page.getByTestId("rf__edge-sequence-1-2");
  const secondToThird = page.getByTestId("rf__edge-sequence-2-3");
  await expect(firstToSecond).toBeVisible();
  await expect(secondToThird).toBeVisible();
  await expect(page.locator(".activity-sequence-edge")).toHaveCount(2);
  await expect(firstToSecond.locator(".react-flow__edge-path")).toHaveAttribute(
    "marker-end",
    /type=arrowclosed/,
  );

  await firstToSecond.locator(".react-flow__edge-interaction").dblclick({ force: true });
  await page.keyboard.press("Delete");
  await expect(page.locator(".activity-sequence-edge")).toHaveCount(2);
  expect(api.state.canvasEdges).toEqual([]);
  expect(edgeMutationRequests).toBe(0);
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

test("the card delete control hides a node before activity deletion finishes", async ({ page, api }) => {
  let releaseDelete!: () => void;
  let markRequested!: () => void;
  const requested = new Promise<void>((resolve) => { markRequested = resolve; });
  const released = new Promise<void>((resolve) => { releaseDelete = resolve; });
  await page.route("**/v1/canvas/11", async (route) => {
    if (route.request().method() !== "DELETE") {
      await route.fallback();
      return;
    }
    markRequested();
    await released;
    await route.fallback();
  });
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  await card.getByRole("button", { name: "활동 기록 삭제" }).click();
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await expect(confirmation).toBeVisible();
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  await requested;

  await expect(card).toHaveCount(0);
  expect(api.state.canvasNodes.some((node) => node.activity_event_id === 1)).toBe(true);
  releaseDelete();
  await expect.poll(() => api.state.canvasNodes.some(
    (node) => node.activity_event_id === 1,
  )).toBe(false);
  expect(api.state.activities.some((activity) => activity.id === 1)).toBe(false);
});

test("canvas activity deletion is cancelled without an API request", async ({ page, api }) => {
  let deleteRequests = 0;
  page.on("request", (request) => {
    if (request.method() === "DELETE" && new URL(request.url()).pathname === "/v1/canvas/11") {
      deleteRequests += 1;
    }
  });
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  await card.getByRole("button", { name: "활동 기록 삭제" }).click();
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await expect(confirmation).toBeVisible();
  await expect(card).toBeVisible();
  await confirmation.getByRole("button", { name: "취소" }).click();

  await expect(confirmation).toHaveCount(0);
  await expect(card).toBeVisible();
  expect(deleteRequests).toBe(0);
  expect(api.state.activities.some((activity) => activity.id === 1)).toBe(true);
});

test("a failed card removal restores the node and explains the failure", async ({ page }) => {
  await page.route("**/v1/canvas/11", async (route) => {
    if (route.request().method() !== "DELETE") {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      headers: { "Access-Control-Allow-Origin": "*" },
      body: JSON.stringify({
        code: "canvas_delete_failed",
        message: "삭제 요청을 처리하지 못했습니다.",
      }),
    });
  });
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  const failed = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/canvas/11");
  await card.getByRole("button", { name: "활동 기록 삭제" }).click();
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  await failed;

  await expect(card).toBeVisible();
  await expect(page.getByRole("alert")).toContainText("삭제 요청을 처리하지 못했습니다.");
});

test("double-clicking a node preserves its canvas placement", async ({ page, api }) => {
    const deletes: string[] = [];
    page.on("request", (request) => {
      if (request.method() === "DELETE") {
        deletes.push(new URL(request.url()).pathname);
      }
    });

    await page.goto("/");
    const card = page.getByTestId("activity-node-2");

    await card.dblclick();
    await expect(page.getByTestId("activity-detail-panel")).toBeVisible();
    await expect(card).toBeVisible();
    expect(deletes).toEqual([]);
    expect(api.state.canvasNodes.some((node) => node.activity_event_id === 2)).toBe(true);
  });

test("double-clicking an edge removes the connection", async ({ page, api }) => {
  await page.goto("/");
  const edge = page.locator(".react-flow__edge").first();
  const deleted = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/canvas/edges/21");

  await edge.locator(".react-flow__edge-interaction").dblclick({ force: true });
  await expect(edge).toHaveCount(0);
  expect((await deleted).status()).toBe(204);
  expect(api.state.canvasNodes).toHaveLength(2);
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

test("keyboard delete removes the selected activity and its canvas node", async ({ page, api }) => {
  await page.goto("/");
  const card = page.getByTestId("activity-node-1");
  await card.click();
  await expect(card.locator("..")).toHaveClass(/selected/);
  const deleted = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/canvas/11", { timeout: 1_000 })
    .catch(() => null);

  await page.keyboard.press("Delete");
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  const response = await deleted;

  expect({
    deleteStatus: response?.status() ?? null,
    canvasNodePresent: api.state.canvasNodes.some((node) => node.activity_event_id === 1),
    activityPresent: api.state.activities.some((activity) => activity.id === 1),
  }).toEqual({
    deleteStatus: 204,
    canvasNodePresent: false,
    activityPresent: false,
  });

  await expect(card).toHaveCount(0);
});
