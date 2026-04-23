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
      // a second instance. `stdout: "pipe"` + `stderr: "pipe"` surface
      // startup output on failure — without them Playwright swallows
      // it and "ERR_CONNECTION_REFUSED" is all you see.
      command: "cargo run -q -p floe-server",
      cwd: "../../",
      port: 8787,
      reuseExistingServer: true,
      timeout: 240_000,
      stdout: "pipe",
      stderr: "pipe",
      env: {
        // In-memory DB → no Postgres dependency just to land the
        // smoke test. Override locally if you want persistence.
        FLOE_DB: ":memory:",
        // Silence GLM/LLM path entirely — the smoke test never hits
        // the pipeline, so no keys are needed.
        FLOE_LLM: "skip",
      },
    },
    {
      // `-- --host 127.0.0.1` forces Vite to bind to ipv4 explicitly.
      // On Actions runners, the default `localhost` bind sometimes
      // resolves to ::1 only, and Playwright's baseURL goes to
      // 127.0.0.1 — the mismatch surfaces as ERR_CONNECTION_REFUSED
      // even though Vite reports "ready".
      command: "npm run dev -- --host 127.0.0.1",
      port: 5173,
      reuseExistingServer: true,
      timeout: 120_000,
      stdout: "pipe",
      stderr: "pipe",
    },
  ],
});
