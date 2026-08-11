import { expect, test as base, type Locator, type Page } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [async ({ page }, use) => use(await installFixtureApi(page)), { auto: true }],
});

test.use({ locale: "ko-KR", timezoneId: "Asia/Seoul" });

const evidence = "../.omo/evidence/task-18-project-context-and-conversation";

function card(page: Page, id: number) {
  return page.getByTestId(`activity-node-${id}`);
}

function panel(page: Page) {
  return page.getByTestId("activity-detail-panel");
}

function assignmentBar(page: Page) {
  return page.getByRole("region").filter({
    has: page.locator('input[name="assignment-destination"]'),
  });
}

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

function setPrompt(api: FixtureApi, activityId: number, prompt: string) {
  api.state.activities.find(({ id }) => id === activityId)!.prompt = prompt;
  for (const detail of Object.values(api.state.details)) {
    if (detail.id === activityId) detail.prompt = prompt;
    const turn = detail.conversation.find(({ id }) => id === activityId);
    if (turn) turn.prompt = prompt;
  }
}

async function openDetail(page: Page, activityId: number) {
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET"
    && new URL(candidate.url()).pathname === `/v1/activities/${activityId}`
    && candidate.status() === 200,
  );
  await card(page, activityId).click();
  await response;
  await expect(panel(page)).toBeVisible();
}

async function tabTo(page: Page, target: Locator, limit = 24) {
  for (let index = 0; index < limit; index += 1) {
    await page.keyboard.press("Tab");
    if (await target.evaluate((element) => element === document.activeElement)) return;
  }
  throw new Error("Expected control was not reachable through the keyboard focus order");
}

async function assertNoHorizontalOverflow(page: Page, element: Locator) {
  const width = await element.evaluate((node) => ({
    client: node.clientWidth,
    scroll: node.scrollWidth,
  }));
  expect(width.scroll, `${element} must not overflow ${JSON.stringify(width)}`)
    .toBeLessThanOrEqual(width.client);
}

test("detail reserves its own navigation-safe column at desktop and narrow widths", async ({ page }) => {
  await page.setViewportSize({ width: 1280, height: 900 });
  await page.goto("/");
  await openDetail(page, 1);

  const desktop = await Promise.all([page.locator(".rail"), panel(page)].map(
    (locator) => locator.evaluate((node) => node.getBoundingClientRect()),
  ));
  expect(desktop[0]!.right).toBeLessThanOrEqual(desktop[1]!.left + 1);
  await page.screenshot({ path: `${evidence}/desktop-detail.png`, fullPage: true });

  await page.setViewportSize({ width: 390, height: 844 });
  const narrow = await Promise.all([page.locator(".rail"), panel(page)].map(
    (locator) => locator.evaluate((node) => node.getBoundingClientRect()),
  ));
  expect(narrow[1]!.left).toBeGreaterThanOrEqual(0);
  expect(narrow[1]!.right).toBeLessThanOrEqual(390);
  expect(narrow[1]!.top).toBeGreaterThanOrEqual(narrow[0]!.bottom - 1);
  await page.screenshot({ path: `${evidence}/narrow-detail.png`, fullPage: true });
});

test("spaced Korean cards use strict CJK typography, exactly three visual lines, and full detail text", async ({ page, api }) => {
  const prompt = "공백이 있는 한국어 문장은 의미 단위로 자연스럽게 줄바꿈되어야 하며 카드에서는 세 줄까지만 보여야 합니다 반복 문장입니다";
  setPrompt(api, 1, prompt);
  await page.goto("/");

  const summary = card(page, 1).locator(".activity-node__prompt");
  const typography = await summary.evaluate((node) => {
    const style = getComputedStyle(node);
    return {
      wordBreak: style.wordBreak,
      lineBreak: style.lineBreak,
      hyphens: style.hyphens,
      lines: Math.round(node.getBoundingClientRect().height / parseFloat(style.lineHeight)),
    };
  });
  await page.screenshot({ path: `${evidence}/spaced-korean-card.png`, fullPage: true });
  expect(typography.wordBreak).toBe("keep-all");
  expect(typography.lineBreak).toBe("strict");
  expect(typography.hyphens).toBe("none");
  expect(typography.lines).toBe(3);

  await openDetail(page, 1);
  expect(await panel(page).locator(".activity-detail__selected > p").textContent()).toBe(prompt);
});

test("unspaced Korean, URLs, and mixed text cannot overflow the document, card, or detail", async ({ page, api }) => {
  const prompt = "공백없는한국어문장과https://example.test/this-is-a-very-long-url-that-must-wrap-without-expanding-the-layout그리고MixedText123";
  setPrompt(api, 2, prompt);
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  await openDetail(page, 2);

  await page.screenshot({ path: `${evidence}/narrow-overflow.png`, fullPage: true });
  await assertNoHorizontalOverflow(page, page.locator("html"));
  await assertNoHorizontalOverflow(page, card(page, 2).locator(".activity-node__prompt"));
  await assertNoHorizontalOverflow(page, panel(page));
  expect(await panel(page).locator(".activity-detail__selected > p").textContent()).toBe(prompt);
});

test("narrow project canvas preserves a readable activity-node width", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 700 });
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("project:1");

  const canvas = page.locator(".flow-stage");
  const activity = canvas.locator(".react-flow__node-activity");
  const [canvasBounds, cardBounds] = await Promise.all([
    canvas.evaluate((node) => node.getBoundingClientRect()),
    activity.evaluate((node) => node.getBoundingClientRect()),
  ]);

  expect(cardBounds.width).toBeGreaterThanOrEqual(288);
  expect(cardBounds.left).toBeGreaterThanOrEqual(canvasBounds.left - 1);
  expect(cardBounds.right).toBeLessThanOrEqual(canvasBounds.right + 1);
  await assertNoHorizontalOverflow(page, activity.locator(".activity-node__prompt"));
  await activity.scrollIntoViewIfNeeded();
  await page.screenshot({
    path: `${evidence}/final-narrow-canvas.png`,
  });
});

test("compact setup and detail panels keep every action reachable by scrolling", async ({ page, api }) => {
  setPrompt(api, 1, "긴 상세 내용입니다.\n".repeat(80));
  await page.setViewportSize({ width: 390, height: 480 });
  await page.goto("/");

  await page
    .getByRole("navigation", { name: "작업 위치" })
    .getByRole("button", { name: /미분류/ })
    .click();
  const setup = page.locator(".dialog-card");
  const setupMetrics = await setup.evaluate((node) => ({
    bottom: node.getBoundingClientRect().bottom,
    clientHeight: node.clientHeight,
    overflowY: getComputedStyle(node).overflowY,
    scrollHeight: node.scrollHeight,
  }));
  expect(setupMetrics.bottom).toBeLessThanOrEqual(480);
  expect(setupMetrics.overflowY).toBe("auto");
  expect(setupMetrics.scrollHeight).toBeGreaterThan(setupMetrics.clientHeight);
  const close = setup.getByRole("button", { name: "닫기" });
  expect(
    await close.evaluate((node) => getComputedStyle(node).whiteSpace),
  ).toBe("nowrap");
  const save = setup.getByRole("button", { name: "설정 저장" });
  await save.scrollIntoViewIfNeeded();
  await expect(save).toBeInViewport();
  const saveBounds = await save.evaluate((node) => node.getBoundingClientRect());
  expect(saveBounds.top).toBeGreaterThanOrEqual(0);
  expect(saveBounds.bottom).toBeLessThanOrEqual(setupMetrics.bottom - 12);
  await page.screenshot({
    path: `${evidence}/final-compact-origin-setup.png`,
  });
  await close.click();

  await openDetail(page, 1);
  const detail = panel(page);
  await detail.scrollIntoViewIfNeeded();
  const detailMetrics = await detail.evaluate((node) => ({
    top: node.getBoundingClientRect().top,
    bottom: node.getBoundingClientRect().bottom,
    clientHeight: node.clientHeight,
    overflowY: getComputedStyle(node).overflowY,
    scrollHeight: node.scrollHeight,
  }));
  expect(detailMetrics.top).toBeGreaterThanOrEqual(-1);
  expect(detailMetrics.bottom).toBeLessThanOrEqual(481);
  expect(detailMetrics.overflowY).toBe("auto");
  expect(detailMetrics.scrollHeight).toBeGreaterThan(detailMetrics.clientHeight);
  await page.screenshot({
    path: `${evidence}/final-compact-detail-top.png`,
  });
  await detail.locator("summary").scrollIntoViewIfNeeded();
  await expect(detail.locator("summary")).toBeInViewport();
  await page.screenshot({
    path: `${evidence}/final-compact-detail-scroll.png`,
  });
});

test("keyboard-only focus order completes origin setup, assignment, technical disclosure, copy, and detail close", async ({ page, api, context }) => {
  sharedInbox(api);
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");

  const setup = page
    .getByRole("navigation", { name: "작업 위치" })
    .getByRole("button", { name: /미분류/ });
  await tabTo(page, setup);
  await page.keyboard.press("Enter");
  const dialog = page.getByRole("dialog");
  await expect(dialog).toBeVisible();
  const modes = dialog.locator('input[type="radio"]');
  await expect(modes.nth(1)).toBeFocused();
  await page.keyboard.press("ArrowUp");
  await page.keyboard.press("ArrowDown");
  const originSaved = page.waitForResponse((candidate) =>
    candidate.request().method() === "PATCH"
    && new URL(candidate.url()).pathname === "/v1/origins/2/routing"
    && candidate.status() === 200,
  );
  await tabTo(page, dialog.locator('button[type="submit"]'));
  await page.keyboard.press("Enter");
  await originSaved;
  await expect(dialog).toHaveCount(0);

  const node = card(page, 2).locator("..");
  const detailLoaded = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET"
    && new URL(candidate.url()).pathname === "/v1/activities/2"
    && candidate.status() === 200,
  );
  await tabTo(page, node);
  await expect(node).toBeFocused();
  expect(await node.evaluate((element) => getComputedStyle(element).outlineStyle))
    .toBe("solid");
  await page.screenshot({ path: `${evidence}/keyboard-node-focus.png`, fullPage: true });
  await page.keyboard.press("Enter");
  await detailLoaded;
  const assignment = assignmentBar(page);
  await expect(assignment).toBeVisible();
  const choices = assignment.locator('input[type="radio"]');
  await tabTo(page, choices.first());
  await page.keyboard.press("Space");
  const assignmentSaved = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/activity-assignments"
    && candidate.status() === 200,
  );
  await tabTo(page, assignment.locator('button[type="submit"]'));
  await page.keyboard.press("Enter");
  await assignmentSaved;

  const detail = panel(page);
  await tabTo(page, detail.locator("summary"));
  await page.keyboard.press("Space");
  const copy = detail.locator(".activity-detail__technical button").first();
  await expect(copy).toBeVisible();
  await tabTo(page, copy);
  await page.keyboard.press("Enter");
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("fixture-session");
  await tabTo(page, detail.locator("header button"));
  await page.keyboard.press("Enter");
  await expect(detail).toHaveCount(0);
  await page.screenshot({ path: `${evidence}/keyboard-lifecycle.png`, fullPage: true });
});

test("a server assignment error preserves keyboard-selected state and destination intent", async ({ page, api }) => {
  sharedInbox(api);
  const before = structuredClone(api.state);
  await page.route("**/v1/activity-assignments", async (route) => {
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "stale", message: "server rejected the assignment" }),
    });
  });
  await page.goto("/");

  const node = card(page, 2).locator("..");
  await tabTo(page, node);
  await page.keyboard.press("Enter");
  const assignment = assignmentBar(page);
  await expect(assignment).toBeVisible();
  const destination = assignment.locator('input[type="radio"]').first();
  await tabTo(page, destination);
  await page.keyboard.press("Space");
  const rejected = page.waitForResponse((candidate) =>
    candidate.request().method() === "POST"
    && new URL(candidate.url()).pathname === "/v1/activity-assignments"
    && candidate.status() === 422,
  );
  await tabTo(page, assignment.locator('button[type="submit"]'));
  await page.keyboard.press("Enter");
  await rejected;

  await expect(page.getByRole("alert")).toHaveText("server rejected the assignment");
  await expect(destination).toBeChecked();
  await expect(node).toHaveClass(/selected/);
  expect(api.state).toEqual(before);
  await page.screenshot({ path: `${evidence}/assignment-error.png`, fullPage: true });
});
