import { expect, test } from "@playwright/test";

/** Smoke test: the anon landing renders and talks to the backend.
 *
 *  Intentionally narrow — a full "pick sample → READY → open flow"
 *  run is pipeline-dependent (LLM keys, probe baselines, Postgres)
 *  and would take 60-120s in Actions. This test just proves the
 *  surface is alive: Vite serves, the landing H1 shows, and the
 *  samples gallery got data from the server. Richer tests land
 *  once the pipeline is reproducibly runnable in CI. */
test("landing renders and samples endpoint responds", async ({ page, request }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toContainText("stories");

  // Backend handshake — GET /samples should return 200 (even empty
  // arrays are fine; all we're asserting is the server's alive).
  const r = await request.get("http://127.0.0.1:8787/samples");
  expect(r.status(), `expected 200 from /samples, got ${r.status()}`).toBe(200);
});
