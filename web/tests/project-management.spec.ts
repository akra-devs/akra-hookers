import { expect, test as base, type Page } from "@playwright/test";
import { join } from "node:path";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [
    async ({ page }, use) => {
      await use(await installFixtureApi(page));
    },
    { auto: true },
  ],
});
const evidencePath = (name: string) =>
  join(process.env.PLAYWRIGHT_EVIDENCE_DIR ?? "../.omo/evidence", name);

function canvasState(api: FixtureApi) {
  return structuredClone({
    nodes: api.state.canvasNodes,
    edges: api.state.canvasEdges,
  });
}

function addTargetProject(api: FixtureApi, name = "연결 대상") {
  const id = api.state.nextProjectId++;
  api.state.projects.push({
    id,
    name,
    origin_count: 0,
    activity_count: 0,
    needs_setup: false,
    latest_activity_at_us: null,
  });
  return id;
}

function escapeRegExp(value: string) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function originLabel(path: string) {
  const segments = path.split(/[\\/]/).filter(Boolean);
  return segments[segments.length - 1] ?? path;
}

async function openOrigin(page: Page, path: string) {
  const navigation = page
    .getByRole("navigation", { name: "작업 위치" })
    .getByRole("button", {
      name: new RegExp(escapeRegExp(originLabel(path))),
    });
  if (await navigation.count() === 1) {
    await navigation.click();
  } else {
    const segments = path.split(/[\\/]/).filter(Boolean);
    const contextualLabel = segments.length >= 3
      ? segments.slice(-2).join("/")
      : originLabel(path);
    await page
      .getByRole("navigation", { name: "작업 위치" })
      .getByRole("button", {
        name: new RegExp(escapeRegExp(contextualLabel)),
      })
      .click();
  }
  await expect(page.getByRole("dialog", {
    name: "이 작업 위치를 어떻게 사용할까요?",
  })).toBeVisible();
}

test("rail exposes every work location with unique path and state text", async ({ page, api }) => {
  api.state.origins.push(
    {
      ...api.state.origins[1]!,
      id: 3,
      display_path: "D:\\work\\두 번째",
      default_project_name: "두 번째 추천",
    },
    {
      ...api.state.origins[0]!,
      id: 4,
      display_path: "D:\\work\\전용",
    },
    {
      ...api.state.origins[0]!,
      id: 5,
      display_path: "C:\\Users\\dorani",
      routing_mode: "shared",
      default_project_id: null,
      default_project_name: null,
    },
  );
  await page.goto("/");

  const locations = page.getByRole("navigation", { name: "작업 위치" });
  await expect(locations.getByRole("button")).toHaveCount(5);
  await expect(
    locations.getByRole("button").filter({ hasText: "설정 필요" }),
  ).toHaveCount(2);
  await expect(
    locations.getByRole("button").filter({ hasText: "확인됨" }),
  ).toHaveCount(3);

  for (const label of [
    "akra-hookers",
    "미분류",
    "두 번째",
    "전용",
    "dorani",
  ]) {
    await expect(
      locations.getByRole("button", { name: new RegExp(label) }),
    ).toBeVisible();
  }
  await expect(locations).not.toContainText("C:\\");
  await expect(locations).not.toContainText("D:\\");
});

test("rail orders stable filters without exposing paths and setup suggests basename", async ({ page, api }) => {
  api.state.origins[1]!.display_path = "D:\\work\\akra-hookers";
  api.state.origins[1]!.recommended_mode = "dedicated";
  api.state.projects[1]!.name = "akra-hookers";
  api.state.origins[1]!.default_project_name = "akra-hookers";
  await page.goto("/");

  await expect(page.getByLabel("프로젝트 필터").locator("option")).toHaveText([
    "All projects",
    "분류 필요 (1)",
    "기존 프로젝트",
    "akra-hookers",
  ]);
  await expect(
    page.getByRole("region", { name: "프로젝트" }),
  ).not.toContainText("C:\\dev\\");
  await expect(page.getByTestId("activity-node-1")).not.toContainText("C:\\dev\\");

  await openOrigin(page, api.state.origins[1]!.display_path);
  await expect(
    page.getByRole("dialog").getByText("D:\\work\\akra-hookers", { exact: true }),
  ).toBeVisible();
  await expect(page.getByLabel("새 프로젝트 이름")).toHaveValue("akra-hookers");
  const dialog = page.locator(".dialog-card");
  const save = dialog.getByRole("button", { name: "설정 저장" });
  const cardBottom = await dialog.evaluate(
    (node) => node.getBoundingClientRect().bottom,
  );
  const buttonBottom = await save.evaluate(
    (node) => node.getBoundingClientRect().bottom,
  );
  expect(buttonBottom).toBeLessThanOrEqual(cardBottom - 12);
  await page.screenshot({
    path: evidencePath("task-14-project-context-and-conversation.png"),
    fullPage: true,
  });
});

test("root or home recommendation preselects shared without persisting", async ({ page, api }) => {
  api.state.origins[1]!.display_path = "C:\\Users\\dorani";
  api.state.origins[1]!.recommended_mode = "shared";
  let routingMutations = 0;
  page.on("request", (request) => {
    if (request.method() === "PATCH" && request.url().endsWith("/v1/origins/2/routing")) {
      routingMutations += 1;
    }
  });
  await page.goto("/");

  await openOrigin(page, api.state.origins[1]!.display_path);
  await expect(page.getByLabel("여러 프로젝트가 함께 쓰는 위치")).toBeChecked();
  await page.screenshot({
    path: evidencePath("task-14-root-shared.png"),
    fullPage: true,
  });
  expect(routingMutations).toBe(0);
  await page.getByRole("button", { name: "닫기" }).click();
  expect(routingMutations).toBe(0);
  expect(api.state.origins[1]!.setup_state).toBe("unconfirmed");
});

test("unchanged suggested project confirms without historical-move confirmation", async ({
  page,
  api,
}) => {
  const origin = api.state.origins[1]!;
  origin.display_path = "D:\\work\\akra-hookers";
  origin.recommended_mode = "dedicated";
  origin.activity_count = 1;
  const before = canvasState(api);
  await page.goto("/");
  await openOrigin(page, origin.display_path);

  await expect(page.getByText(/기존 활동 .*이동합니다/)).toHaveCount(0);

  const request = page.waitForRequest((candidate) =>
    candidate.method() === "PATCH"
    && candidate.url().endsWith("/v1/origins/2/routing"));
  await page.getByRole("button", { name: "설정 저장" }).click();
  expect((await request).postDataJSON()).toEqual({
    mode: "dedicated",
    destination: { new_project_name: "미분류" },
    confirm: true,
  });
  await expect(page.getByRole("dialog")).toHaveCount(0);
  expect(api.state.projects).toHaveLength(2);
  expect(origin).toMatchObject({
    setup_state: "confirmed",
    default_project_id: 2,
    default_project_name: "미분류",
  });
  expect(canvasState(api)).toEqual(before);
});

test("renaming a suggested project preserves its ID before confirming", async ({
  page,
  api,
}) => {
  const origin = api.state.origins[1]!;
  origin.display_path = "D:\\work\\akra-hookers";
  origin.recommended_mode = "dedicated";
  origin.activity_count = 1;
  const before = canvasState(api);
  const mutations: Array<{ path: string; body: unknown }> = [];
  page.on("request", (request) => {
    if (request.method() === "PATCH") {
      mutations.push({
        path: new URL(request.url()).pathname,
        body: request.postDataJSON(),
      });
    }
  });
  await page.goto("/");
  await openOrigin(page, origin.display_path);

  await page.getByRole("button", { name: "이름 직접 바꾸기" }).click();
  await page.getByLabel("새 프로젝트 이름").fill("새 작업 프로젝트");
  await page.getByRole("button", { name: "설정 저장" }).click();
  await expect(page.getByRole("dialog")).toHaveCount(0);

  expect(mutations).toEqual([
    {
      path: "/v1/origins/2/routing",
      body: {
        mode: "dedicated",
        destination: { new_project_name: "새 작업 프로젝트" },
        confirm: true,
      },
    },
  ]);
  expect(api.state.projects).toHaveLength(2);
  expect(api.state.projects[1]).toMatchObject({
    id: 2,
    name: "새 작업 프로젝트",
  });
  expect(origin).toMatchObject({
    setup_state: "confirmed",
    default_project_id: 2,
    default_project_name: "새 작업 프로젝트",
  });
  expect(canvasState(api)).toEqual(before);
});

test("setup connects an existing project and confirms affected dedicated history", async ({ page, api }) => {
  const targetId = addTargetProject(api);
  const before = canvasState(api);
  await page.goto("/");
  await openOrigin(page, api.state.origins[0]!.display_path);
  await page.getByLabel("기존 프로젝트 연결").check();
  await page.getByLabel("연결할 프로젝트").selectOption(String(targetId));
  await expect(page.getByText("기존 활동 2개와 이후 활동이 이동합니다.")).toBeVisible();
  await page.screenshot({
    path: evidencePath("task-14-connect-existing.png"),
    fullPage: true,
  });
  await expect(page.getByRole("button", { name: "설정 저장" })).toBeDisabled();
  await page.getByLabel("기존 활동 이동을 확인합니다").check();

  const request = page.waitForRequest((candidate) =>
    candidate.method() === "PATCH"
    && candidate.url().endsWith("/v1/origins/1/routing"));
  await page.getByRole("button", { name: "설정 저장" }).click();
  expect((await request).postDataJSON()).toEqual({
    mode: "dedicated",
    destination: { project_id: targetId },
    confirm: true,
  });
  expect(
    api.state.activities.every((activity) => activity.project?.id === targetId),
  ).toBe(true);
  expect(canvasState(api)).toEqual(before);
});

test("failed suggested rename leaves project and origin unchanged", async ({ page, api }) => {
  const origin = api.state.origins[1]!;
  origin.display_path = "D:\\work\\akra-hookers";
  origin.recommended_mode = "dedicated";
  const projectBefore = structuredClone(api.state.projects[1]!);
  await page.route("**/v1/origins/2/routing", async (route) => {
    if (route.request().method() !== "PATCH") {
      await route.fallback();
      return;
    }
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "routing_failed", message: "라우팅 저장 실패" }),
    });
  });
  await page.goto("/");
  await openOrigin(page, origin.display_path);
  await page.getByRole("button", { name: "이름 직접 바꾸기" }).click();
  await page.getByLabel("새 프로젝트 이름").fill("실패할 이름");

  await page.getByRole("button", { name: "설정 저장" }).click();

  await expect(page.getByRole("alert")).toContainText("라우팅 저장 실패");
  await expect(page.getByRole("dialog")).toBeVisible();
  expect(api.state.projects[1]).toEqual(projectBefore);
  expect(origin.setup_state).toBe("unconfirmed");
  expect(origin.default_project_id).toBe(projectBefore.id);
  expect(origin.default_project_name).toBe(projectBefore.name);
});

test("new rename and explicit merge keep stable filter IDs and canvas state", async ({ page, api }) => {
  const before = canvasState(api);
  const targetId = api.state.nextProjectId;
  await page.goto("/");
  await page.getByRole("button", { name: "새 프로젝트" }).click();
  await page.getByRole("textbox", { name: "프로젝트 이름", exact: true }).fill("병합 대상");
  await page.getByRole("button", { name: "프로젝트 만들기" }).click();
  await page.getByLabel("프로젝트 필터").selectOption("project:1");
  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByRole("textbox", { name: "프로젝트 이름", exact: true }).fill("바뀐 이름");
  await page.getByRole("button", { name: "이름 저장" }).click();
  await expect(page.getByLabel("프로젝트 필터")).toHaveValue("project:1");
  await expect(page.getByLabel("프로젝트 필터").locator("option:checked")).toHaveText("바뀐 이름");

  await page.getByRole("button", { name: "프로젝트 관리" }).click();
  await page.getByLabel("병합 대상").selectOption(String(targetId));
  await page.getByRole("button", { name: "병합..." }).click();
  await page.screenshot({
    path: evidencePath("task-14-merge-confirmation.png"),
    fullPage: true,
  });
  await page.getByRole("button", { name: "병합 확인" }).click();

  await expect(page.getByLabel("프로젝트 필터")).toHaveValue(`project:${targetId}`);
  await expect(page.getByLabel("프로젝트 필터").locator("option")).toHaveText([
    "All projects",
    "분류 필요 (1)",
    "미분류",
    "병합 대상",
  ]);
  expect(api.state.activities[0]!.project).toEqual({
    id: targetId,
    name: "병합 대상",
  });
  expect(canvasState(api)).toEqual(before);
});

test("409 keeps a new-project dialog and input without optimistic state", async ({ page, api }) => {
  const before = structuredClone(api.state);
  await page.route("**/v1/projects", async (route) => {
    if (route.request().method() !== "POST") return route.fallback();
    await route.fulfill({
      status: 409,
      contentType: "application/json",
      body: JSON.stringify({ code: "project_name_conflict", message: "이미 사용 중인 이름입니다." }),
    });
  });
  await page.goto("/");
  await page.getByRole("button", { name: "새 프로젝트" }).click();
  await page.getByRole("textbox", { name: "프로젝트 이름", exact: true }).fill("중복 이름");
  await page.getByRole("button", { name: "프로젝트 만들기" }).click();

  await expect(page.getByRole("alert")).toHaveText("이미 사용 중인 이름입니다.");
  await expect(page.getByRole("dialog")).toBeVisible();
  await page.screenshot({
    path: evidencePath("task-14-inline-error.png"),
    fullPage: true,
  });
  await expect(page.getByRole("textbox", {
    name: "프로젝트 이름",
    exact: true,
  })).toHaveValue("중복 이름");
  expect(api.state).toEqual(before);
});

test("422 keeps origin setup open and preserves all server-backed state", async ({ page, api }) => {
  api.state.origins[1]!.display_path = "D:\\work\\akra-hookers";
  api.state.origins[1]!.recommended_mode = "dedicated";
  const before = structuredClone(api.state);
  await page.route("**/v1/origins/2/routing", async (route) => {
    await route.fulfill({
      status: 422,
      contentType: "application/json",
      body: JSON.stringify({ code: "invalid_transition", message: "전환할 수 없습니다." }),
    });
  });
  await page.goto("/");
  await openOrigin(page, api.state.origins[1]!.display_path);
  await page.getByLabel("새 프로젝트 만들기").check();
  await page.getByLabel("새 프로젝트 이름").fill("입력 유지");
  await page.getByRole("button", { name: "설정 저장" }).click();

  await expect(page.getByRole("alert")).toHaveText("전환할 수 없습니다.");
  await expect(page.getByRole("dialog")).toBeVisible();
  await expect(page.getByLabel("새 프로젝트 이름")).toHaveValue("입력 유지");
  expect(api.state).toEqual(before);
});

test("project and origin dialogs contain focus and restore their triggers", async ({ page }) => {
  await page.goto("/");
  const newProject = page.getByRole("button", { name: "새 프로젝트" });
  await newProject.click();
  const projectName = page.getByRole("textbox", {
    name: "프로젝트 이름",
    exact: true,
  });
  await expect(projectName).toBeFocused();
  await projectName.fill("키보드 프로젝트");
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "닫기" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "프로젝트 만들기" })).toBeFocused();
  await page.getByRole("button", { name: "닫기" }).click();
  await expect(newProject).toBeFocused();

  const setup = page
    .getByRole("navigation", { name: "작업 위치" })
    .getByRole("button", { name: /미분류/ });
  await setup.click();
  await expect(page.getByLabel("여러 프로젝트가 함께 쓰는 위치")).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "닫기" })).toBeFocused();
  await page.keyboard.press("Shift+Tab");
  await expect(page.getByRole("button", { name: "설정 저장" })).toBeFocused();
  await page.getByRole("button", { name: "닫기" }).click();
  await expect(setup).toBeFocused();
});
