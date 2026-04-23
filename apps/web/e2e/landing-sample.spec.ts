/* eslint-disable */
// @ts-nocheck — @playwright/test is an optional dev dep; install
// with `npm i -D @playwright/test` in apps/web when you want to run
// the e2e suite. The import stays resolvable at runtime under
// `npx playwright test` even when not present in tsc's program.
import { expect, test } from "@playwright/test";

/**
 * Happy-path smoke test for the landing → sample → flow opens flow.
 * No auth — runs against the anon landing, picks the first sample,
 * waits for the pipeline to go READY, then verifies the ship-readiness
 * card renders and a flow tab opens its workspace.
 */
test("anon reviewer opens a sample through to a flow view", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("heading", { level: 1 })).toContainText("stories");

  const sampleButtons = page.locator("[data-testid=sample-card-button]");
  // Fallback if the samples gallery doesn't use a testid — click the
  // first "Try" button in the gallery section.
  const firstSample = (await sampleButtons.count()) > 0
    ? sampleButtons.first()
    : page.getByRole("button").filter({ hasText: /try|run/i }).first();
  await firstSample.click();

  // Pipeline progress renders a per-stage list; wait for it to
  // disappear or for the ship-readiness card to appear.
  await expect(
    page.getByText(/ship[-\s]?readiness|ready to ship|request changes/i),
  ).toBeVisible({ timeout: 120_000 });

  // Click the first flow tab in the top spine.
  const flowTab = page.getByRole("button").filter({ hasText: /flow\s|top-?level/i }).first();
  if (await flowTab.isVisible()) {
    await flowTab.click();
    // Expect either an entity graph or its mobile-fallback list.
    await expect(page.locator("svg, ul").first()).toBeVisible();
  }
});
