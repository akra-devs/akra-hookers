import { expect, test as base, type Page } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [async ({ page }, use) => use(await installFixtureApi(page)), { auto: true }],
});

test.use({ locale: "ko-KR", timezoneId: "Asia/Seoul" });

function panel(page: Page) {
  return page.getByTestId("activity-detail-panel");
}

async function open(page: Page, activityId: number) {
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET"
    && new URL(candidate.url()).pathname === `/v1/activities/${activityId}`
    && candidate.status() === 200,
  );
  await page.getByTestId(`activity-node-${activityId}`).click();
  await response;
  return panel(page);
}

function setConversation(api: FixtureApi) {
  const { activities, details, projects, canvasNodes } = api.state;
  projects.push({
    id: 2, name: "두 번째 프로젝트", origin_count: 0, activity_count: 1,
    needs_setup: false, latest_activity_at_us: null,
  });
  activities[0]!.prompt = "가장 먼저 기록된 한국어 프롬프트";
  activities[0]!.time = { value: "2026-08-08T10:00:00Z", provenance: "captured" };
  activities[1]!.prompt = "가운데 선택 프롬프트의 전체 내용";
  activities[1]!.project = { id: 2, name: "두 번째 프로젝트" };
  activities[1]!.time = { value: "2026-08-08T12:00:00Z", provenance: "captured" };
  const last = structuredClone(activities[1]!);
  last.id = 3;
  last.prompt = "마지막 한국어 프롬프트의 전체 내용";
  last.project = null;
  last.time = { value: "2026-08-08T14:00:00Z", provenance: "captured" };
  last.conversation_index = 3;
  activities.push(last);
  canvasNodes.push({ id: 13, activity_event_id: 3, position_x: 720, position_y: 160 });
  details[3] = { ...structuredClone(details[2]!), id: 3, prompt: last.prompt, project: last.project };
  syncDetails(api);
}

function insertEarlierTurn(api: FixtureApi) {
  const earlier = structuredClone(api.state.activities[0]!);
  earlier.id = 4;
  earlier.prompt = "지연되어 도착한 가장 이른 프롬프트";
  earlier.time = { value: "2026-08-08T08:00:00Z", provenance: "captured" };
  api.state.activities.unshift(earlier);
  api.state.details[4] = {
    ...structuredClone(api.state.details[1]!), id: 4, prompt: earlier.prompt,
    project: earlier.project, captured_at: earlier.time, first_recorded_at: earlier.time,
  };
  syncDetails(api);
}

function syncDetails(api: FixtureApi) {
  const { activities, details, canvasNodes } = api.state;
  const visible = new Set(canvasNodes.map((node) => node.activity_event_id));
  for (const activity of activities) {
    activity.conversation_total = activities.length;
    const detail = details[activity.id];
    if (detail) {
      detail.prompt = activity.prompt;
      detail.project = activity.project;
      detail.captured_at = activity.time;
      detail.first_recorded_at = activity.time;
      detail.on_canvas = visible.has(activity.id);
      detail.selected_turn = {
        id: activity.id,
        activity_kind: activity.activity_kind,
        prompt: activity.prompt,
        project: activity.project,
        time: activity.time,
        on_canvas: visible.has(activity.id),
        selected: true,
        result_summary: activity.id === detail.id
          ? detail.result_summary
          : { status: "unavailable", lines: null },
        prompt_summary: activity.id === detail.id
          ? detail.prompt_summary
          : activity.prompt_summary,
      };
      detail.conversation_index = activities.findIndex(({ id }) => id === activity.id) + 1;
      detail.conversation_total = activities.length;
      detail.conversation_has_more = false;
    }
  }
  for (const detail of Object.values(details)) {
    detail.conversation = activities.map((activity) => ({
      id: activity.id, activity_kind: activity.activity_kind,
      prompt: activity.prompt, project: activity.project, time: activity.time,
      on_canvas: visible.has(activity.id), selected: activity.id === detail.id,
      result_summary: activity.id === detail.id
        ? detail.result_summary
        : { status: "unavailable", lines: null },
      prompt_summary: activity.id === detail.id
        ? detail.prompt_summary
        : activity.prompt_summary,
    }));
  }
}

test("conversation pages load explicitly without duplicate turns", async ({ page, api }) => {
  const detail = api.state.details[1]!;
  const initial = structuredClone(detail.conversation[0]!);
  const later = structuredClone(detail.conversation[1]!);
  later.result_summary = { status: "pending", lines: null };
  detail.conversation = [initial];
  detail.conversation_total = 2;
  detail.conversation_has_more = true;
  await page.route("**/v1/activities/1?*", async (route) => {
    const url = new URL(route.request().url());
    if (url.searchParams.get("conversation_after_id") !== String(initial.id)) {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 200,
      contentType: "application/json",
      body: JSON.stringify({
        ...detail,
        conversation: [later],
        conversation_has_more: false,
      }),
    });
  });
  await page.goto("/");
  const detailPanel = await open(page, 1);
  await expect(detailPanel.getByRole("heading", { name: "대화 기록 (1/2)" })).toBeVisible();

  await detailPanel.getByRole("button", { name: "대화 기록 더 보기" }).click();

  await expect(detailPanel.getByRole("heading", { name: "대화 기록 (2/2)" })).toBeVisible();
  await expect(detailPanel.locator(".activity-detail__turn")).toHaveCount(2);
  await expect(detailPanel.getByRole("button", { name: "대화 기록 더 보기" })).toHaveCount(0);
  await expect(detailPanel.getByText("RES · 결과 요약 중", { exact: true })).toBeVisible();

  later.result_summary = {
    status: "ready",
    lines: ["갱신된 첫 줄", "갱신된 둘째 줄", "갱신된 셋째 줄"],
  };
  await expect(detailPanel.getByText("갱신된 첫 줄", { exact: true })).toBeVisible({
    timeout: 2_500,
  });
});

test("a selected turn outside the first conversation page is visible immediately", async ({
  page,
  api,
}) => {
  const detail = api.state.details[2]!;
  detail.conversation = [structuredClone(api.state.details[1]!.selected_turn)];
  detail.conversation_total = 2;
  detail.conversation_has_more = true;
  await page.goto("/");
  const detailPanel = await open(page, 2);

  await expect(
    detailPanel.locator("[data-activity-id='2'][aria-current='true']"),
  ).toBeVisible();
  await expect(detailPanel.getByRole("heading", { name: "대화 기록 (2/2)" })).toBeVisible();
});

test("a card opens a right detail column with truthful Korean metadata and guarded technical values", async ({ page, api, context }) => {
  const detail = api.state.details[2]!;
  detail.prompt = "전체 한국어 프롬프트는 카드에서 잘려도 상세 패널에서는 한 글자도 생략되지 않습니다.";
  detail.submitted_cwd = "C:\\submitted\\worktree";
  detail.origin.display_path = "C:\\detected\\repository";
  detail.technical = {
    session_id: "SESSION_DETAIL_ONLY",
    turn_id: "TURN_DETAIL_ONLY",
    agent_id: null,
    agent_type: null,
  };
  await context.grantPermissions(["clipboard-read", "clipboard-write"]);
  await page.goto("/");
  await open(page, 2);
  await page.screenshot({ path: "../.omo/evidence/task-16-project-context-and-conversation.png", fullPage: true });

  const detailPanel = panel(page);
  await expect(detailPanel).toBeVisible();
  await expect(detailPanel).toContainText(detail.prompt);
  await expect(detailPanel).toContainText("Inbox");
  await expect(detailPanel).toContainText("codex");
  await expect(detailPanel.getByTestId("captured-at")).toHaveAttribute("data-provenance", "captured");
  await expect(detailPanel.getByTestId("captured-at").locator("time")).toHaveAttribute("datetime", "2026-08-08T12:00:00Z");
  await expect(detailPanel.getByText("C:\\submitted\\worktree", { exact: true })).toBeVisible();
  await expect(detailPanel.getByText("C:\\detected\\repository", { exact: true })).toBeVisible();
  await expect(detailPanel.locator("details").filter({ hasText: "기술 정보" })).not.toHaveAttribute("open", "");
  await expect(detailPanel).not.toContainText("SESSION_DETAIL_ONLY");
  await expect(page.locator(".rail")).not.toContainText("SESSION_DETAIL_ONLY");
  await expect(page.locator(".canvas-panel")).not.toContainText("SESSION_DETAIL_ONLY");
  await detailPanel.getByText("기술 정보", { exact: true }).click();
  await detailPanel.getByRole("button", { name: "세션 ID 복사" }).click();
  expect(await page.evaluate(() => navigator.clipboard.readText())).toBe("SESSION_DETAIL_ONLY");
  const [rail, canvas, side] = await Promise.all([".rail", ".canvas-panel", "[data-testid=activity-detail-panel]"].map(
    async (selector) => page.locator(selector).evaluate((element) => element.getBoundingClientRect()),
  ));
  expect(canvas.right).toBeLessThanOrEqual(side.left + 1);
  expect(side.left).toBeGreaterThanOrEqual(rail.right - 1);
});

test("a failed detail request renders an alert and supports an explicit retry", async ({ page }) => {
  let rejectDetail = true;
  await page.route("**/v1/activities/1?*", async (route) => {
    if (!rejectDetail) {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 500,
      contentType: "application/json",
      body: JSON.stringify({
        code: "detail_unavailable",
        message: "상세 조회 실패",
      }),
    });
  });
  await page.goto("/");
  const failed = page.waitForResponse((response) =>
    response.request().method() === "GET"
    && new URL(response.url()).pathname === "/v1/activities/1"
    && response.status() === 500,
  );
  await page.getByTestId("activity-node-1").click();
  await failed;

  const detailPanel = panel(page);
  await expect(detailPanel.getByRole("alert")).toContainText(
    "활동 상세를 불러오지 못했습니다",
  );
  await expect(detailPanel).not.toContainText("불러오는 중입니다");
  await expect(
    detailPanel.getByRole("button", { name: "상세 닫기" }),
  ).toBeVisible();
  await page.screenshot({
    path: "../.omo/evidence/final-detail-error.png",
    fullPage: true,
  });

  rejectDetail = false;
  const recovered = page.waitForResponse((response) =>
    response.request().method() === "GET"
    && new URL(response.url()).pathname === "/v1/activities/1"
    && response.status() === 200,
  );
  await detailPanel.getByRole("button", { name: "다시 시도" }).click();
  await recovered;
  await expect(detailPanel.getByRole("heading", { name: "활동 상세" })).toBeVisible();
});

test("legacy time and a missing submitted path remain explicitly truthful", async ({ page, api }) => {
  const detail = api.state.details[2]!;
  detail.submitted_cwd = null;
  detail.captured_at = { value: "2026-08-08T12:00:00Z", provenance: "legacy_recorded" };
  detail.first_recorded_at = { value: null, provenance: "unknown" };
  detail.origin.resolution_source = "legacy_migrated";
  await page.goto("/");
  const detailPanel = await open(page, 2);

  await expect(detailPanel.getByTestId("submitted-cwd")).toHaveText("정확한 작업 경로를 사용할 수 없음");
  await expect(detailPanel.getByTestId("captured-at")).toHaveAttribute("data-provenance", "legacy_recorded");
  await expect(detailPanel.getByTestId("captured-at")).not.toContainText("기존 기록");
  await expect(detailPanel.getByTestId("first-recorded-at")).toHaveAttribute("data-provenance", "unknown");
  await expect(detailPanel.getByTestId("first-recorded-at")).toContainText("시간 정보 없음");
  await expect(detailPanel.getByTestId("detected-path")).toHaveAttribute("data-resolution-source", "legacy_migrated");
});

test("the detail panel stacks without horizontal overflow on a narrow Korean viewport", async ({ page }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");
  const detailPanel = await open(page, 2);

  await expect(detailPanel).toBeVisible();
  const bounds = await detailPanel.evaluate((element) => element.getBoundingClientRect());
  expect(bounds.left).toBeGreaterThanOrEqual(0);
  expect(bounds.right).toBeLessThanOrEqual(390);
  expect(await page.evaluate(() => document.documentElement.scrollWidth))
    .toBeLessThanOrEqual(await page.evaluate(() => document.documentElement.clientWidth));

  await detailPanel.getByRole("button", { name: "대화 기록 크게 보기" }).click();
  const dialog = page.getByRole("dialog", { name: "대화 흐름" });
  const dialogBounds = await dialog.evaluate((element) => element.getBoundingClientRect());
  const closeBounds = await dialog.getByRole("button", { name: "대화 흐름 닫기" })
    .evaluate((element) => element.getBoundingClientRect());
  expect(dialogBounds.left).toBeGreaterThanOrEqual(0);
  expect(dialogBounds.right).toBeLessThanOrEqual(390);
  expect(closeBounds.width).toBeGreaterThanOrEqual(44);
  expect(closeBounds.height).toBeGreaterThanOrEqual(44);
  expect(await page.evaluate(() => document.documentElement.scrollWidth))
    .toBeLessThanOrEqual(await page.evaluate(() => document.documentElement.clientWidth));
  await page.screenshot({
    path: "../.omo/evidence/conversation-flow-mobile.png",
  });
});

test("a ready Spark result renders exactly three stored lines without expanding the node", async ({
  page,
  api,
}) => {
  const lines = [
    "결과 캡처와 프롬프트를 같은 턴으로 연결했습니다.",
    "요약 작업은 격리된 Codex Spark 실행기로 처리합니다.",
    "전체 저장 및 UI 계약 검증을 통과했습니다.",
  ] as [string, string, string];
  expect(Array.from(lines.join("")).length).toBeLessThanOrEqual(180);
  api.state.activities[0]!.result_summary_status = "ready";
  api.state.details[1]!.result_summary = { status: "ready", lines };
  api.state.details[1]!.selected_turn.result_summary = { status: "ready", lines };
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  await expect(card.getByText("요약 있음", { exact: true })).toBeVisible();
  await expect(card).not.toContainText(lines[0]);
  const detailPanel = await open(page, 1);
  const result = detailPanel.getByTestId("activity-result-summary");
  await expect(result).toHaveAttribute("data-status", "ready");
  await expect(result.locator("li")).toHaveCount(3);
  await expect(result.locator("li")).toHaveText(lines);
  await expect(detailPanel.getByText("결과 요약 보기", { exact: true })).toHaveCount(0);
});

test("contextual prompt summaries replace wrapper-heavy node copy while the raw request stays disclosed", async ({
  page,
  api,
}) => {
  const raw = "<in-app-browser-context source=\"ambient-ui-state\">\nmetadata\n</in-app-browser-context>\n\n## My request:\n진행해";
  const summary = {
    status: "ready" as const,
    mode: "contextual" as const,
    text: "이전 검증 결과를 반영해 구현을 계속 진행한다",
  };
  api.state.activities[0]!.prompt = raw;
  api.state.activities[0]!.prompt_summary = summary;
  api.state.details[1]!.prompt = raw;
  api.state.details[1]!.prompt_summary = summary;
  api.state.details[1]!.selected_turn.prompt = raw;
  api.state.details[1]!.selected_turn.prompt_summary = summary;
  api.state.details[1]!.conversation[0]!.prompt = raw;
  api.state.details[1]!.conversation[0]!.prompt_summary = summary;
  await page.goto("/");

  const card = page.getByTestId("activity-node-1");
  await expect(card).toContainText(summary.text);
  await expect(card.getByText("문맥 보강", { exact: true })).toBeVisible();
  await expect(card).not.toContainText("ambient-ui-state");

  const detailPanel = await open(page, 1);
  await expect(detailPanel.locator(".activity-detail__selected .expandable-prompt > p"))
    .toHaveText(summary.text);
  const rawDisclosure = detailPanel.locator(".activity-detail__raw-prompt");
  await expect(rawDisclosure).not.toHaveAttribute("open", "");
  await rawDisclosure.getByText("수집된 원문 보기", { exact: true }).click();
  await expect(rawDisclosure).toHaveAttribute("open", "");
  await expect(rawDisclosure).toContainText(raw);
  await expect(detailPanel.locator(".activity-detail__turn-request").first()).toContainText(summary.text);
});

test("pending and failed result states remain non-blocking and update through polling", async ({
  page,
  api,
}) => {
  api.state.activities[0]!.result_summary_status = "pending";
  api.state.details[1]!.result_summary = { status: "pending", lines: null };
  api.state.details[1]!.selected_turn.result_summary = { status: "pending", lines: null };
  await page.goto("/");
  const detailPanel = await open(page, 1);
  const result = detailPanel.getByTestId("activity-result-summary");
  await expect(result).toHaveAttribute("data-status", "pending");
  await expect(result).toHaveAttribute("aria-busy", "true");
  await expect(result).toContainText("Codex Spark가 결과를 요약하는 중입니다.");

  api.state.activities[0]!.result_summary_status = "failed";
  api.state.details[1]!.result_summary = { status: "failed", lines: null };
  api.state.details[1]!.selected_turn.result_summary = { status: "failed", lines: null };
  await expect(result).toHaveAttribute("data-status", "failed", { timeout: 2_000 });
  await expect(result).toContainText("결과 요약을 만들지 못했습니다.");
  await expect(detailPanel.getByRole("heading", { name: "대화 기록 (2/2)" })).toBeVisible();
  await expect(page.getByTestId("activity-node-1")).toContainText("요약 실패");
});

test("a historical turn keeps its request/result preview compact and remains keyboard selectable", async ({ page, api }) => {
  const lines = ["첫 줄", "둘째 줄", "셋째 줄"] as [string, string, string];
  const historical = api.state.details[1]!.conversation.find(({ id }) => id === 2)!;
  historical.result_summary = { status: "ready", lines };
  await page.goto("/");
  const detailPanel = await open(page, 1);
  const historicalTurn = detailPanel.locator("[data-activity-id='2']");
  await expect(historicalTurn.locator(".activity-detail__turn-result")).toContainText("첫 줄");
  await expect(historicalTurn.locator(".activity-detail__turn-result")).toContainText("+2");
  await expect(historicalTurn).not.toContainText("둘째 줄");
  const selection = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET"
    && new URL(candidate.url()).pathname === "/v1/activities/2",
  );
  await historicalTurn.getByRole("button").focus();
  await page.keyboard.press("Enter");
  await selection;
  await expect(panel(page)).toHaveAttribute("data-selected-activity-id", "2");
});

test("conversation history opens as a focused, expanded flow and restores focus on close", async ({
  page,
  api,
}) => {
  setConversation(api);
  const lines = [
    "확대 화면에서는 첫 번째 결과 줄을 모두 보여 줍니다.",
    "두 번째 결과도 좁은 패널처럼 생략하지 않습니다.",
    "세 번째 결과까지 한 흐름 안에서 확인할 수 있습니다.",
  ] as [string, string, string];
  api.state.details[2]!.conversation.find(({ id }) => id === 1)!.result_summary = {
    status: "ready",
    lines,
  };
  await page.goto("/");
  const detailPanel = await open(page, 2);
  const expand = detailPanel.getByRole("button", { name: "대화 기록 크게 보기" });

  await expand.click();

  const dialog = page.getByRole("dialog", { name: "대화 흐름" });
  await expect(dialog).toBeVisible();
  await expect(dialog.locator("#conversation-flow-description"))
    .toContainText("오래된 기록부터 · 1/1 페이지 · 총 3개");
  await expect(dialog.locator(".activity-conversation-dialog__timeline > li")).toHaveCount(3);
  await expect(dialog.locator("[data-activity-id='2']")).toHaveAttribute("aria-current", "true");
  await expect(dialog.locator(".activity-conversation-dialog__result-lines > span"))
    .toHaveText(lines);
  await expect(dialog.getByRole("button", { name: "대화 흐름 닫기" })).toBeFocused();

  const [dialogBounds, panelBounds] = await Promise.all([
    dialog.evaluate((element) => element.getBoundingClientRect()),
    detailPanel.evaluate((element) => element.getBoundingClientRect()),
  ]);
  expect(dialogBounds.width).toBeGreaterThan(panelBounds.width * 2);
  expect(Math.abs(dialogBounds.left + dialogBounds.width / 2 - 640)).toBeLessThanOrEqual(1);
  await page.screenshot({
    path: "../.omo/evidence/conversation-flow-desktop.png",
    fullPage: true,
  });

  await page.keyboard.press("Escape");
  await expect(dialog).toHaveCount(0);
  await expect(expand).toBeFocused();
});

test("long request prompts stay at four lines until each view is expanded", async ({ page, api }) => {
  const prompt = Array.from(
    { length: 10 },
    (_, index) => `요청 프롬프트 ${index + 1}번째 줄을 확인합니다.`,
  ).join("\n");
  api.state.activities[1]!.prompt = prompt;
  syncDetails(api);
  await page.goto("/");
  const detailPanel = await open(page, 2);

  const selectedPrompt = detailPanel.locator(".activity-detail__selected .expandable-prompt");
  const selectedText = selectedPrompt.locator("p");
  await expect(selectedPrompt.getByRole("button", { name: "더 보기" })).toBeVisible();
  const collapsed = await selectedText.evaluate((element) => ({
    clientHeight: element.clientHeight,
    scrollHeight: element.scrollHeight,
  }));
  expect(collapsed.scrollHeight).toBeGreaterThan(collapsed.clientHeight);
  await selectedPrompt.getByRole("button", { name: "더 보기" }).click();
  await expect(selectedPrompt.getByRole("button", { name: "접기" }))
    .toHaveAttribute("aria-expanded", "true");
  expect(await selectedText.evaluate((element) => element.clientHeight))
    .toBeGreaterThan(collapsed.clientHeight);

  await detailPanel.getByRole("button", { name: "대화 기록 크게 보기" }).click();
  const selectedTurn = page.getByRole("dialog", { name: "대화 흐름" })
    .locator("[data-activity-id='2']");
  const modalPrompt = selectedTurn.locator(".activity-conversation-dialog__prompt");
  await expect(modalPrompt.getByRole("button", { name: "더 보기" })).toBeVisible();
  await modalPrompt.getByRole("button", { name: "더 보기" }).click();
  await expect(modalPrompt.getByRole("button", { name: "접기" }))
    .toHaveAttribute("aria-expanded", "true");
});

test("the expanded flow opens on the selected page and supports previous and next paging", async ({
  page,
  api,
}) => {
  setConversation(api);
  for (let id = 4; id <= 18; id += 1) {
    const activity = structuredClone(api.state.activities[1]!);
    activity.id = id;
    activity.prompt = `페이지 검증 대화 ${id}`;
    activity.time = {
      value: `2026-08-${String(8 + Math.floor(id / 8)).padStart(2, "0")}T${String(id % 24).padStart(2, "0")}:00:00Z`,
      provenance: "captured",
    };
    api.state.activities.push(activity);
    api.state.details[id] = {
      ...structuredClone(api.state.details[2]!),
      id,
      prompt: activity.prompt,
      project: activity.project,
    };
    api.state.canvasNodes.push({
      id: 100 + id,
      activity_event_id: id,
      position_x: 100 + id * 20,
      position_y: 180,
    });
  }
  syncDetails(api);
  await page.goto("/");
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET"
    && new URL(candidate.url()).pathname === "/v1/activities/14"
    && candidate.status() === 200,
  );
  await page.getByTestId("activity-node-14").dispatchEvent("click");
  await response;
  const detailPanel = panel(page);
  await detailPanel.getByRole("button", { name: "대화 기록 크게 보기" }).click();
  const dialog = page.getByRole("dialog", { name: "대화 흐름" });

  await expect(dialog.locator("#conversation-flow-description"))
    .toContainText("2/3 페이지 · 총 18개");
  await expect(dialog.locator(".activity-conversation-dialog__timeline > li")).toHaveCount(8);
  await expect(dialog.locator("[data-activity-id='14']")).toHaveAttribute("aria-current", "true");

  await dialog.getByRole("button", { name: "이전" }).click();
  await expect(dialog.locator("#conversation-flow-description")).toContainText("1/3 페이지");
  await expect(dialog.locator("[data-activity-id='1']")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "이전" })).toBeDisabled();

  await dialog.getByRole("button", { name: "다음" }).click();
  await expect(dialog.locator("#conversation-flow-description")).toContainText("2/3 페이지");
  await dialog.getByRole("button", { name: "다음" }).click();
  await expect(dialog.locator("#conversation-flow-description")).toContainText("3/3 페이지");
  await expect(dialog.locator(".activity-conversation-dialog__timeline > li")).toHaveCount(2);
  await expect(dialog.locator("[data-activity-id='18']")).toBeVisible();
  await expect(dialog.getByRole("button", { name: "다음" })).toBeDisabled();
});

test("an unwanted historical log is deleted from the modal after confirmation", async ({
  page,
  api,
}) => {
  setConversation(api);
  await page.goto("/");
  const detailPanel = await open(page, 2);
  await detailPanel.getByRole("button", { name: "대화 기록 크게 보기" }).click();
  const flow = page.getByRole("dialog", { name: "대화 흐름" });
  const deleted = page.waitForResponse((candidate) =>
    candidate.request().method() === "DELETE"
    && new URL(candidate.url()).pathname === "/v1/activities/1"
    && candidate.status() === 204,
  );
  await flow.locator("[data-activity-id='1']")
    .getByRole("button", { name: "활동 기록 1 삭제" }).click();
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await expect(confirmation).toContainText("활동 목록, 대화 흐름, Canvas에서 사라집니다");
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  await deleted;

  await expect(confirmation).toHaveCount(0);
  await expect(flow).toBeVisible();
  await expect(flow.locator("[data-activity-id='1']")).toHaveCount(0);
  await expect(flow.locator("#conversation-flow-description")).toContainText("총 2개");
  expect(api.state.activities.map(({ id }) => id)).toEqual([2, 3]);
  expect(api.state.canvasNodes.map(({ activity_event_id }) => activity_event_id)).toEqual([2, 3]);
});

test("the selected activity can be deleted from the detail header", async ({ page, api }) => {
  await page.goto("/");
  const detailPanel = await open(page, 2);
  const deleted = page.waitForResponse((candidate) =>
    candidate.request().method() === "DELETE"
    && new URL(candidate.url()).pathname === "/v1/activities/2"
    && candidate.status() === 204,
  );
  await detailPanel.getByRole("button", { name: "이 활동 기록 삭제" }).click();
  const confirmation = page.getByRole("alertdialog", { name: "활동 기록을 삭제할까요?" });
  await confirmation.getByRole("button", { name: "기록 삭제" }).click();
  await deleted;

  await expect(panel(page)).toHaveCount(0);
  await expect(page.getByTestId("activity-node-2")).toHaveCount(0);
  expect(api.state.details[2]).toBeUndefined();
  expect(api.state.activities.map(({ id }) => id)).toEqual([1]);
});

test("a long selected prompt and bounded result preserve a usable conversation viewport", async ({
  page,
  api,
}) => {
  await page.setViewportSize({ width: 1280, height: 720 });
  api.state.details[1]!.prompt = Array.from(
    { length: 36 },
    (_, index) => `Long selected prompt line ${index + 1}: 상세 기록을 계속 확인합니다.`,
  ).join("\n");
  const longResult = [
    "핵심 완료 사항과 사용자 영향을 한 문장으로 간결하게 정리했습니다.",
    "중요한 변경과 판단 근거는 불필요한 세부 없이 짧게 기록했습니다.",
    "검증 결과와 남은 주의점을 포함하면서 전체 180자 제한을 지켰습니다.",
  ] as [string, string, string];
  expect(Array.from(longResult.join("")).length).toBeLessThanOrEqual(180);
  api.state.details[1]!.result_summary = { status: "ready", lines: longResult };
  api.state.details[1]!.selected_turn.result_summary = { status: "ready", lines: longResult };
  await page.goto("/");
  const detailPanel = await open(page, 1);

  const metrics = await detailPanel.evaluate((element) => {
    const context = element.querySelector<HTMLElement>(".activity-detail__context")!;
    const timeline = element.querySelector<HTMLElement>(".activity-detail__timeline")!;
    const list = timeline.querySelector<HTMLOListElement>("ol")!;
    const turns = Array.from(list.querySelectorAll<HTMLElement>(".activity-detail__turn"));
    const firstTurn = turns[0]!.getBoundingClientRect();
    const nextTurn = turns[1]!.getBoundingClientRect();
    return {
      contextClientHeight: context.clientHeight,
      contextScrollHeight: context.scrollHeight,
      listHeight: list.getBoundingClientRect().height,
      panelBottom: element.getBoundingClientRect().bottom,
      timelineBottom: timeline.getBoundingClientRect().bottom,
      firstTurnHeight: firstTurn.height,
      firstTurnScrollHeight: turns[0]!.scrollHeight,
      firstTurnBottom: firstTurn.bottom,
      nextTurnTop: nextTurn.top,
    };
  });

  expect(metrics.contextScrollHeight).toBeGreaterThan(metrics.contextClientHeight);
  expect(metrics.listHeight).toBeGreaterThanOrEqual(220);
  expect(metrics.timelineBottom).toBeLessThanOrEqual(metrics.panelBottom + 1);
  expect(metrics.firstTurnHeight).toBeGreaterThanOrEqual(metrics.firstTurnScrollHeight - 1);
  expect(metrics.nextTurnTop).toBeGreaterThanOrEqual(metrics.firstTurnBottom - 1);
});

test("a long timeline reveals its initially selected immutable turn", async ({ page, api }) => {
  const detail = api.state.details[2]!;
  const template = detail.conversation[0]!;
  const selected = detail.conversation.find(({ id }) => id === 2)!;
  detail.conversation = [
    ...Array.from({ length: 10 }, (_, index) => ({
      ...structuredClone(template),
      id: 100 + index,
      prompt: `앞선 대화 ${index + 1}`,
      selected: false,
      on_canvas: false,
    })),
    { ...selected, selected: true },
  ];
  await page.goto("/");
  const detailPanel = await open(page, 2);
  const visible = await detailPanel.locator('[data-activity-id="2"]').evaluate((element) => {
    const list = element.closest("ol")!;
    const listBounds = list.getBoundingClientRect();
    const turnBounds = element.getBoundingClientRect();
    return turnBounds.top >= listBounds.top && turnBounds.bottom <= listBounds.bottom;
  });

  expect(visible).toBe(true);
});

test("the oldest-first timeline retains its selected ID and scroll anchor across a delayed earlier response and rename", async ({ page, api }) => {
  setConversation(api);
  await page.goto("/");
  const detailPanel = await open(page, 2);
  await expect(detailPanel).toBeVisible();
  const timeline = detailPanel.getByRole("list", { name: "대화 기록" });
  await expect(timeline.locator("[data-activity-id]").first()).toHaveAttribute("data-activity-id", "1");
  await expect(timeline).toContainText("가장 먼저 기록된 한국어 프롬프트");
  await expect(timeline).toContainText("가운데 선택 프롬프트의 전체 내용");
  await expect(timeline).toContainText("마지막 한국어 프롬프트의 전체 내용");
  await page.getByLabel("프로젝트 필터").selectOption("project:2");
  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByLabel("프로젝트 이름", { exact: true }).fill("이름이 바뀐 프로젝트");
  const delayed = api.deferNextDetail(2);
  const response = page.waitForResponse((candidate) =>
    candidate.request().method() === "GET" && new URL(candidate.url()).pathname === "/v1/activities/2" && candidate.status() === 200,
  );
  await page.getByRole("button", { name: "이름 저장" }).click();
  await delayed.requested;
  const selected = timeline.locator('[data-activity-id="2"]');
  await selected.scrollIntoViewIfNeeded();
  const anchorTop = await selected.evaluate((element) => element.getBoundingClientRect().top);
  insertEarlierTurn(api);
  delayed.release();
  await response;
  await expect(detailPanel).toHaveAttribute("data-selected-activity-id", "2");
  await expect(timeline.locator("[data-activity-id]").first()).toHaveAttribute("data-activity-id", "4");
  expect(await selected.evaluate((element) => element.getBoundingClientRect().top)).toBeCloseTo(anchorTop, 0);
  await expect(detailPanel).toContainText("이름이 바뀐 프로젝트");
});

test("the pane closes only when its selected node becomes invisible while deleted history stays immutable", async ({ page, api }) => {
  const origin = api.state.origins[0]!;
  origin.routing_mode = "shared";
  origin.default_project_id = null;
  origin.default_project_name = null;
  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  let detailPanel = await open(page, 2);
  await expect(detailPanel).toBeVisible();
  const assignment = page.getByRole("region", { name: "프로젝트에 배정" });
  await assignment.getByLabel("기존 프로젝트").check();
  await assignment.getByLabel("프로젝트", { exact: true }).selectOption("1");
  const assigned = page.waitForResponse((candidate) => candidate.request().method() === "POST" && new URL(candidate.url()).pathname === "/v1/activity-assignments");
  await assignment.getByRole("button", { name: "배정 저장" }).click();
  await assigned;
  await expect(panel(page)).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  detailPanel = await open(page, 2);
  const deleted = page.waitForResponse((candidate) => candidate.request().method() === "DELETE" && new URL(candidate.url()).pathname === "/v1/canvas/12");
  await page.keyboard.press("Backspace");
  await deleted;
  await expect(panel(page)).toHaveCount(0);
  await expect(page.getByTestId("activity-node-2")).toHaveCount(0);
  detailPanel = await open(page, 1);
  await expect(detailPanel.getByRole("list", { name: "대화 기록" }).locator('[data-activity-id="2"]')).toContainText("캔버스에 없음");
  expect(api.state.canvasNodes.map((node) => node.activity_event_id)).toEqual([1]);
  await page.getByLabel("프로젝트 필터").selectOption("inbox");
  await expect(panel(page)).toHaveCount(0);
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await open(page, 1);
  const cleared = page.waitForResponse((candidate) => candidate.request().method() === "DELETE" && new URL(candidate.url()).pathname === "/v1/canvas");
  await page.getByRole("button", { name: "Clear canvas" }).click();
  await page.getByRole("button", { name: "Canvas 비우기" }).click();
  await cleared;
  await expect(panel(page)).toHaveCount(0);
});
