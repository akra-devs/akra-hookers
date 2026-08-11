import { expect, test as base } from "@playwright/test";

import { FixtureApi, installFixtureApi } from "./fixtures/api";

const test = base.extend<{ api: FixtureApi }>({
  api: [
    async ({ page }, use) => {
      await use(await installFixtureApi(page));
    },
    { auto: true },
  ],
});

test("new activity polling keeps the older-history cursor at its loaded boundary", async ({
  page,
  api,
}) => {
  const activity = api.state.activities[1]!;
  for (let id = 3; id <= 201; id += 1) {
    api.state.activities.push({
      ...structuredClone(activity),
      id,
      prompt: `prompt ${id}`,
      conversation_index: id,
      conversation_total: 201,
    });
  }
  await page.goto("/");

  const firstOlderResponse = page.waitForResponse((response) => {
    const url = new URL(response.url());
    return url.pathname === "/v1/activities"
      && url.searchParams.get("after_id") === "102";
  });
  await page.getByRole("button", { name: "이전 활동 불러오기" }).click();
  await firstOlderResponse;

  const refreshedActivities = page.waitForResponse(async (response) => {
    const url = new URL(response.url());
    if (
      url.pathname !== "/v1/activities"
      || url.searchParams.has("after_id")
    ) return false;
    const body = await response.json() as Array<{ id: number }>;
    return body[0]?.id === 202;
  });
  api.state.activities.push({
    ...structuredClone(activity),
    id: 202,
    prompt: "prompt 202",
    conversation_index: 202,
    conversation_total: 202,
  });
  await refreshedActivities;

  const nextOlderResponse = page.waitForResponse((response) =>
    new URL(response.url()).searchParams.has("after_id")
  );
  await page.getByRole("button", { name: "이전 활동 불러오기" }).click();
  const nextCursor = new URL((await nextOlderResponse).url()).searchParams.get("after_id");

  expect(nextCursor).toBe("2");
  await expect(page.getByTestId("activity-node-1")).toBeVisible();
});
