import { defineConfig, devices } from "@playwright/test";

// E2E config. The smoke test assumes the dev stack is already running (just up).
// CI will start the stack as a step before invoking playwright.
const BASE_URL = process.env.SPRINTLY_E2E_BASE_URL ?? "http://localhost:8080";

export default defineConfig({
  testDir: "./tests",
  fullyParallel: false, // tests share a DB; keep them serial in v1
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? "github" : "list",
  use: {
    baseURL: BASE_URL,
    trace: "on-first-retry",
    // The HttpOnly auth cookie needs explicit context wiring. Default is fine.
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  timeout: 30_000,
});
