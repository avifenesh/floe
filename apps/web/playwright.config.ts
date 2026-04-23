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
      // Playwright manages both processes. `reuseExistingServer: true`
      // means a dev running `just dev` in another shell doesn't force
      // a second instance. In CI we let Playwright spawn the binary —
      // upstream `cargo run` backgrounding was racey.
      command: "cargo run -q -p floe-server",
      cwd: "../../",
      port: 8787,
      reuseExistingServer: true,
      timeout: 240_000,
    },
    {
      command: "npm run dev",
      port: 5173,
      reuseExistingServer: true,
      timeout: 120_000,
    },
  ],
});
