import { defineConfig, devices } from "@playwright/test";

/**
 * Playwright config — one happy-path test for the sample landing flow.
 * Runs the Vite dev server + floe-server automatically via webServer,
 * so CI can invoke `npx playwright test` with no extra orchestration.
 */
export default defineConfig({
  testDir: "./e2e",
  timeout: 60_000,
  expect: { timeout: 10_000 },
  fullyParallel: false,
  reporter: process.env.CI ? "github" : [["list"]],
  use: {
    baseURL: "http://127.0.0.1:5173",
    trace: "retain-on-failure",
  },
  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],
  webServer: [
    {
      // The server is expected to already be running in CI — we spawn
      // it via the Rust binary here for local runs.
      command:
        process.env.CI === "true"
          ? "echo 'server expected to be running'"
          : "cargo run -q -p floe-server",
      cwd: "../../",
      port: 8787,
      reuseExistingServer: true,
      timeout: 180_000,
    },
    {
      command: "npm run dev",
      port: 5173,
      reuseExistingServer: true,
      timeout: 60_000,
    },
  ],
});
