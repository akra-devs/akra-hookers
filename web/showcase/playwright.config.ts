import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig, devices } from "@playwright/test";

const showcaseDirectory = dirname(fileURLToPath(import.meta.url));
const webDirectory = resolve(showcaseDirectory, "..");

export default defineConfig({
  testDir: showcaseDirectory,
  testMatch: "record.spec.ts",
  outputDir: resolve(webDirectory, "../artifacts/showcase-playwright"),
  fullyParallel: false,
  workers: 1,
  timeout: 180_000,
  expect: { timeout: 8_000 },
  reporter: "line",
  use: {
    ...devices["Desktop Chrome"],
    baseURL: "http://127.0.0.1:4174",
    channel: "chrome",
    colorScheme: "dark",
    locale: "ko-KR",
    timezoneId: "Asia/Seoul",
    viewport: { width: 2560, height: 1440 },
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    video: {
      mode: "on",
      size: { width: 2560, height: 1440 },
    },
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1 --port 4174 --strictPort",
    cwd: webDirectory,
    env: {
      VITE_AKRA_URL: "http://127.0.0.1:4174",
      VITE_AKRA_TOKEN: "fixture-token",
    },
    port: 4174,
    reuseExistingServer: false,
  },
});
