import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

import { defineConfig, devices } from "@playwright/test";

const webDirectory = dirname(fileURLToPath(import.meta.url));

export default defineConfig({
  testDir: "./tests",
  outputDir: resolve(webDirectory, "../.omo/evidence/task-12-playwright"),
  fullyParallel: false,
  timeout: 10_000,
  expect: { timeout: 5_000 },
  reporter: "line",
  use: {
    baseURL: "http://127.0.0.1:4173",
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
    video: "off",
  },
  webServer: {
    command: "npm run dev -- --host 127.0.0.1 --port 4173 --strictPort",
    cwd: webDirectory,
    env: {
      VITE_AKRA_URL: "http://127.0.0.1:4173",
      VITE_AKRA_TOKEN: "fixture-token",
    },
    port: 4173,
    reuseExistingServer: false,
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"], channel: "chrome" },
    },
  ],
});
