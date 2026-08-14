import { expect, test as base } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [async ({ page }, use) => use(await installFixtureApi(page)), { auto: true }],
});

test("remote collection is explicit, write-only for secrets, and does not restart hooks", async ({
  page,
  api,
}) => {
  await page.goto("/");

  await expect(page.getByTestId("collector-mode")).toHaveText("LOCAL");
  await expect(page.getByTestId("collector-endpoint")).toHaveText("http://127.0.0.1:42130");
  await expect(page.getByText("Captured data stays in this device's Akra data directory.")).toBeVisible();
  await expect(page.getByLabel("Collector URL")).toHaveCount(0);

  await page.getByRole("button", { name: "Change destination" }).click();
  const endpoint = page.getByLabel("Collector URL");
  const save = page.getByRole("button", { name: "Save destination" });
  await expect(endpoint).toBeFocused();

  await endpoint.fill("http://collector.example.com");
  await endpoint.blur();
  await expect(page.getByRole("alert")).toHaveText("외부 수집 주소는 HTTPS를 사용해야 합니다.");

  await endpoint.fill("https://collector.example.com");
  const token = page.getByLabel("Collector access token");
  await expect(token).toBeVisible();
  await save.click();
  await expect(page.getByRole("alert")).toHaveText(
    "새 원격 수집 주소에는 collector access token이 필요합니다.",
  );

  let configuredBody: unknown;
  page.on("request", (request) => {
    if (
      request.method() === "PUT"
      && new URL(request.url()).pathname === "/v1/providers/codex/collector"
    ) {
      configuredBody = request.postDataJSON();
    }
  });
  await token.fill("collector-secret");
  await page.getByRole("button", { name: "Show collector access token" }).click();
  await expect(token).toHaveAttribute("type", "text");
  await page.getByRole("button", { name: "Hide collector access token" }).click();
  await expect(token).toHaveAttribute("type", "password");
  await save.click();

  await expect.poll(() => configuredBody).toEqual({
    endpoint: "https://collector.example.com",
    token: "collector-secret",
  });
  expect(api.state.provider.collector).toMatchObject({
    mode: "remote",
    endpoint: "https://collector.example.com",
    token_configured: true,
    connected: false,
  });
  await expect(page.getByTestId("collector-mode")).toHaveText("REMOTE");
  await expect(page.getByTestId("collector-status")).toHaveText("Not verified");
  await expect(page.getByText("Access token saved")).toBeVisible();
  await expect(page.getByLabel("Collector access token")).toHaveCount(0);
  await expect(page.getByText("Capture hooks를 다시 시작할 필요가 없습니다.")).toBeVisible();
  await expect(page.getByRole("button", { name: /Capture health: Needs check/ })).toBeVisible();

  const verified = page.waitForResponse((response) =>
    response.request().method() === "POST"
    && new URL(response.url()).pathname === "/v1/providers/codex/collector/verify",
  );
  await page.getByRole("button", { name: "Verify connection" }).click();
  await verified;
  await expect(page.getByText("Collector 연결을 확인했습니다.")).toBeVisible();
  await expect(page.getByRole("button", { name: /Capture health: Partial/ })).toBeVisible();

  await page.getByRole("button", { name: "Change destination" }).click();
  await endpoint.fill("https://second.example.com");
  await save.click();
  await expect(page.getByRole("alert")).toHaveText(
    "새 원격 수집 주소에는 collector access token이 필요합니다.",
  );
  await page.getByRole("button", { name: "Cancel" }).click();
  await expect(page.getByRole("button", { name: "Change destination" })).toBeFocused();
});

test("collection destination wraps its compact controls without horizontal overflow", async ({
  page,
}) => {
  await page.setViewportSize({ width: 390, height: 844 });
  await page.goto("/");

  await page.getByRole("button", { name: "Change destination" }).click();
  await page.getByLabel("Collector URL").fill("https://collector.example.com");
  await expect(page.getByLabel("Collector access token")).toBeVisible();
  expect(await page.locator("html").evaluate(
    (element) => element.scrollWidth <= window.innerWidth,
  )).toBe(true);

  await page.getByLabel("Collector access token").fill("narrow-token");
  await page.getByRole("button", { name: "Show collector access token" }).click();
  expect(await page.locator("html").evaluate(
    (element) => element.scrollWidth <= window.innerWidth,
  )).toBe(true);
});
