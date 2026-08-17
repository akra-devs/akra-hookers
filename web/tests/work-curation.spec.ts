import { expect, test as base } from "@playwright/test";

import type { CurationLog, WorkItemDetail } from "../src/api";
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

function readyLog(
  id: number,
  prompt: string,
  summary: string,
  result: [string, string, string],
): CurationLog {
  return {
    id,
    project: { id: 1, name: "기존 프로젝트" },
    time: { value: "2026-08-17T09:00:00Z", provenance: "captured" },
    prompt,
    prompt_summary: { status: "ready", mode: "contextual", text: summary },
    result_summary: { status: "ready", lines: result, can_regenerate: false },
    state: "unreviewed",
  };
}

function workItem(id: number, title: string, log: CurationLog, x: number): WorkItemDetail {
  const source = {
    id: log.id,
    time: log.time,
    prompt: log.prompt,
    prompt_summary: log.prompt_summary,
    result_summary: log.result_summary,
  };
  return {
    id,
    project: log.project,
    title,
    log_count: 1,
    position_x: x,
    position_y: 140,
    updated_at_us: 1_787_046_400_000_000 + id,
    preview_logs: [source],
    logs: [source],
  };
}

test("selected summaries stay inert until the user confirms a work proposal", async ({ page, api }) => {
  const first = readyLog(
    1,
    "RAW_SECRET_1: release-page 전체 구현 맥락과 긴 원문",
    "배포 페이지 공개 준비",
    ["실제 앱 캡처를 배치했습니다.", "다운로드 경로를 연결했습니다.", "반응형 검증을 마쳤습니다."],
  );
  const second = readyLog(
    3,
    "RAW_SECRET_2: 이어서 태그와 다운로드 페이지를 처리해",
    "릴리스 태그와 다운로드 연결",
    ["릴리스 태그를 생성했습니다.", "ZIP 자산을 게시했습니다.", "다운로드 링크를 검증했습니다."],
  );
  api.state.curationLogs.push(first, second);

  const template = structuredClone(api.state.activities[0]!);
  api.state.activities[0] = {
    ...template,
    prompt: first.prompt,
    prompt_summary: first.prompt_summary,
    result_summary_status: "ready",
  };
  api.state.activities.push({
    ...template,
    id: 3,
    prompt: second.prompt,
    prompt_summary: second.prompt_summary,
    result_summary_status: "ready",
    conversation_index: 3,
    conversation_total: 3,
  });
  api.state.details[1]!.prompt = first.prompt;
  api.state.details[1]!.prompt_summary = first.prompt_summary;
  api.state.details[1]!.result_summary = first.result_summary;
  api.state.details[3] = {
    ...structuredClone(api.state.details[1]!),
    id: 3,
    prompt: second.prompt,
    prompt_summary: second.prompt_summary,
    result_summary: second.result_summary,
    technical: { ...api.state.details[1]!.technical, turn_id: "turn-3" },
    selected_turn: {
      ...structuredClone(api.state.details[1]!.selected_turn),
      id: 3,
      prompt: second.prompt,
      prompt_summary: second.prompt_summary,
      result_summary: second.result_summary,
    },
  };

  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "로그 정리" }).click();

  const workspace = page.getByTestId("curation-workspace");
  await expect(workspace).toBeVisible();
  await expect(workspace).toContainText("원본 기록은 보존");
  await expect(workspace).toContainText(
    "최대 96자 요청·3줄 결과만 전송 · 전체 원문/응답 제외 · 최대 20개",
  );
  await expect(workspace.getByText(first.prompt, { exact: true })).not.toBeVisible();
  await expect(workspace.getByText(second.prompt, { exact: true })).not.toBeVisible();
  await workspace.locator(".curation-log__details").first().getByText("더보기", { exact: true }).click();
  await expect(workspace.getByText(first.prompt, { exact: true })).toBeVisible();
  await expect(workspace.getByRole("list", { name: "저장된 결과 요약" })).toBeVisible();
  await page.screenshot({
    path: "../.omo/evidence/task-work-curation-selection.png",
    fullPage: false,
  });

  await workspace.getByRole("checkbox", { name: "배포 페이지 공개 준비 선택" }).check();
  await workspace.getByRole("checkbox", { name: "릴리스 태그와 다운로드 연결 선택" }).check();
  await expect(workspace.getByText("2개 선택", { exact: true })).toBeVisible();

  await workspace.getByRole("button", { name: "선택한 2개 자동 묶기" }).click();
  await expect(workspace.getByRole("heading", { name: "AI가 제안한 작업 묶음" })).toBeVisible();
  expect(api.state.workItems).toHaveLength(0);

  const title = workspace.locator(".curation-group input");
  await title.fill("배포 페이지 공개");
  await expect(workspace).toContainText("AI는 관계선을 만들거나 로그를 삭제하지 않습니다.");
  await workspace.getByRole("button", { name: "검토한 제안 적용" }).click();

  await expect(workspace).toContainText("1개 작업을 Project Memory에 반영했습니다.");
  expect(api.state.workItems).toHaveLength(1);
  expect(api.state.workItems[0]).toMatchObject({ title: "배포 페이지 공개", log_count: 2 });
  expect(api.state.workEdges).toHaveLength(0);

  await workspace.getByRole("button", { name: "작업 지도에서 보기" }).click();
  const workNode = page.getByTestId("work-node-1");
  await expect(workNode).toBeVisible();
  await expect(workNode).toContainText("배포 페이지 공개");
  await expect(workNode).toContainText("LOG 2");
  await expect(workNode).toContainText("사용자 확인");

  await workNode.click();
  const detail = page.getByTestId("work-detail-panel");
  await expect(detail).toContainText("근거 로그");
  await expect(detail.getByRole("list", { name: "결과 요약" })).toHaveCount(2);
  await expect(detail).toContainText("실제 앱 캡처를 배치했습니다.");
  const frame = await page.evaluate(() => ({
    commandHeight: document.querySelector(".command-bar")?.getBoundingClientRect().height,
    commandBottom: document.querySelector(".command-bar")?.getBoundingClientRect().bottom,
    canvasTop: document.querySelector(".canvas-panel")?.getBoundingClientRect().top,
    railTop: document.querySelector(".rail")?.getBoundingClientRect().top,
    detailTop: document.querySelector(".work-detail")?.getBoundingClientRect().top,
    scrollX: window.scrollX,
  }));
  expect(frame.commandHeight).toBe(64);
  expect(frame.canvasTop).toBe(frame.commandBottom);
  expect(frame.railTop).toBe(frame.commandBottom);
  expect(frame.detailTop).toBe(frame.commandBottom);
  expect(frame.scrollX).toBe(0);
  await page.screenshot({
    path: "../.omo/evidence/task-work-memory-detail.png",
    fullPage: false,
  });
});

test("excluded logs are kept separately from permanent soft deletion", async ({ page, api }) => {
  api.state.curationLogs.push(readyLog(
    1,
    "원문은 보존되는 로그",
    "검토에서 잠시 제외할 로그",
    ["첫 결과", "둘째 결과", "셋째 결과"],
  ));

  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "로그 정리" }).click();
  const workspace = page.getByTestId("curation-workspace");

  await workspace.getByRole("button", { name: "이번 정리에서 제외" }).click();
  expect(api.state.curationLogs[0]?.state).toBe("excluded");
  await workspace.getByRole("button", { name: "제외됨" }).click();
  await expect(workspace.getByText("검토에서 잠시 제외할 로그")).toBeVisible();
  await workspace.getByRole("button", { name: "정리 대상으로 복원" }).click();
  expect(api.state.curationLogs[0]?.state).toBe("unreviewed");
});

test("curation offers bounded select-all, red deletion, evidence disclosure, and async regeneration", async ({
  page,
  api,
}) => {
  const ready = readyLog(
    30,
    "REQ 원문: 배포 페이지의 실제 요청",
    "배포 페이지 요청 정리",
    ["응답 첫 줄", "응답 둘째 줄", "응답 셋째 줄"],
  );
  const failed = readyLog(
    31,
    "REQ 원문: 결과 요약을 다시 만들어 주세요",
    "재생성 실패 요약",
    ["unused", "unused", "unused"],
  );
  failed.result_summary = { status: "failed", lines: null, can_regenerate: true };
  const unavailable = readyLog(
    32,
    "REQ 원문: 보존 기간이 지난 기록",
    "원 응답이 없는 과거 요약",
    ["unused", "unused", "unused"],
  );
  unavailable.result_summary = { status: "unavailable", lines: null, can_regenerate: false };
  api.state.curationLogs.push(ready, failed, unavailable);

  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "로그 정리" }).click();
  const workspace = page.getByTestId("curation-workspace");

  const selectAll = workspace.getByRole("checkbox", { name: /전체 선택/ });
  await selectAll.check();
  await expect(workspace.getByText("3개 선택", { exact: true })).toBeVisible();
  await workspace.getByRole("checkbox", { name: "배포 페이지 요청 정리 선택" }).uncheck();
  expect(await selectAll.evaluate((element: HTMLInputElement) => element.indeterminate)).toBe(true);
  await selectAll.check();
  await expect(workspace.getByText("3개 선택", { exact: true })).toBeVisible();

  const deleteButton = workspace.getByRole("button", { name: "로그 영구 제외" }).first();
  await expect(deleteButton).toHaveCSS("color", "rgb(213, 116, 114)");

  const firstDetails = workspace.locator(".curation-log__details").first();
  await expect(workspace.getByText(ready.prompt, { exact: true })).not.toBeVisible();
  await firstDetails.getByText("더보기", { exact: true }).click();
  await expect(workspace.getByText(ready.prompt, { exact: true })).toBeVisible();
  await expect(firstDetails.getByRole("list", { name: "저장된 결과 요약" })).toContainText(
    "응답 셋째 줄",
  );

  const regenerate = workspace.getByRole("button", {
    name: "재생성 실패 요약 결과 요약 재생성",
  });
  const failedRow = workspace.locator(".curation-log").filter({ hasText: "재생성 실패 요약" });
  await expect(regenerate).toBeVisible();
  await expect(workspace.getByRole("button", {
    name: "원 응답이 없는 과거 요약 결과 요약 재생성",
  })).toHaveCount(0);
  await regenerate.click();
  await expect(workspace.locator(".curation-log__regenerate")).toContainText("로딩...");
  await expect(failedRow.locator(".curation-log__result")).toContainText(
    "보관 중인 응답으로 결과 요약을 다시 만들었습니다.",
  );
  await expect(regenerate).toHaveCount(0);
});

test("work relationships are created and removed only through explicit canvas actions", async ({ page, api }) => {
  const first = readyLog(10, "첫 로그", "배포 페이지 구현", ["구현", "검증", "완료"]);
  const second = readyLog(11, "둘째 로그", "릴리스 게시", ["태그", "자산", "게시"]);
  first.state = "organized";
  second.state = "organized";
  api.state.curationLogs.push(first, second);
  api.state.workItems.push(
    workItem(1, "배포 페이지 구현", first, 80),
    workItem(2, "릴리스 게시", second, 480),
  );
  api.state.workRevision = 1;

  await page.goto("/");
  await page.getByRole("button", { name: "작업 지도" }).click();
  await expect(page.getByTestId("work-node-1")).toBeVisible();
  await expect(page.getByTestId("work-node-2")).toBeVisible();
  expect(api.state.workEdges).toHaveLength(0);

  const source = page.getByTestId("work-node-1").locator(".react-flow__handle.source");
  const target = page.getByTestId("work-node-2").locator(".react-flow__handle.target");
  const sourceBox = await source.boundingBox();
  const targetBox = await target.boundingBox();
  expect(sourceBox).not.toBeNull();
  expect(targetBox).not.toBeNull();
  if (!sourceBox || !targetBox) throw new Error("Work handles must be measurable");

  const created = page.waitForResponse((response) =>
    response.request().method() === "POST"
    && new URL(response.url()).pathname === "/v1/work-items/edges",
  );
  await page.mouse.move(sourceBox.x + sourceBox.width / 2, sourceBox.y + sourceBox.height / 2);
  await page.mouse.down();
  await page.mouse.move(targetBox.x + targetBox.width / 2, targetBox.y + targetBox.height / 2);
  await page.mouse.up();
  expect((await created).status()).toBe(201);
  await expect(page.locator(".project-memory-stage .react-flow__edge")).toHaveCount(1);
  expect(api.state.workEdges).toHaveLength(1);

  const edge = page.locator(".project-memory-stage .react-flow__edge").first();
  await edge.locator(".react-flow__edge-interaction").click({ force: true });
  await expect(edge).toHaveClass(/selected/);
  const deleted = page.waitForResponse((response) =>
    response.request().method() === "DELETE"
    && new URL(response.url()).pathname === "/v1/work-items/edges/1",
  );
  await page.keyboard.press("Delete");
  expect((await deleted).status()).toBe(204);
  await expect(page.locator(".project-memory-stage .react-flow__edge")).toHaveCount(0);
  expect(api.state.workEdges).toEqual([]);
  expect(api.state.workItems).toHaveLength(2);
});

test("curation review and work evidence stay inside a narrow viewport", async ({ page, api }) => {
  await page.setViewportSize({ width: 390, height: 844 });
  api.state.curationLogs.push(
    readyLog(20, "긴 원문 A", "프로젝트 작업 A를 정리", ["결과 A1", "결과 A2", "결과 A3"]),
    readyLog(21, "긴 원문 B", "프로젝트 작업 B를 정리", ["결과 B1", "결과 B2", "결과 B3"]),
  );

  await page.goto("/");
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "로그 정리" }).click();
  const workspace = page.getByTestId("curation-workspace");
  await workspace.locator(".curation-log__details").first().getByText("더보기", { exact: true }).click();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  await workspace.getByRole("checkbox", { name: "프로젝트 작업 A를 정리 선택" }).check();
  await workspace.getByRole("checkbox", { name: "프로젝트 작업 B를 정리 선택" }).check();
  await workspace.getByRole("button", { name: "선택한 2개 자동 묶기" }).click();
  await expect(workspace.locator(".curation-group")).toBeVisible();

  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  const group = await workspace.locator(".curation-group").boundingBox();
  expect(group).not.toBeNull();
  if (!group) throw new Error("Curation group must be measurable");
  expect(group.x).toBeGreaterThanOrEqual(0);
  expect(group.x + group.width).toBeLessThanOrEqual(390);

  await workspace.getByRole("button", { name: "검토한 제안 적용" }).click();
  await workspace.getByRole("button", { name: "작업 지도에서 보기" }).click();
  await page.getByTestId("work-node-1").click();
  const detail = page.getByTestId("work-detail-panel");
  await detail.scrollIntoViewIfNeeded();
  await expect(detail).toBeVisible();
  expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(390);
  const detailBox = await detail.boundingBox();
  expect(detailBox).not.toBeNull();
  if (!detailBox) throw new Error("Work detail must be measurable");
  expect(detailBox.x).toBeGreaterThanOrEqual(0);
  expect(detailBox.x + detailBox.width).toBeLessThanOrEqual(390);
  await expect(detail.getByRole("button", { name: "작업 상세 닫기" })).toHaveCSS("height", "44px");
});
