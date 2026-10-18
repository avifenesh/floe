import { expect, test, type Page } from "@playwright/test";
import fixture from "./fixtures/mermaid-rendering.json" with { type: "json" };

const nodeLabels = ["Request", "Allowed", "Render SVG", "Fallback"];
const edgeLabels = ["validate", "yes", "no"];
const browserErrors = new WeakMap<Page, { consoleErrors: string[]; pageErrors: string[] }>();
const invalidSource = fixture.job.artifact.flows[0].membership.diagrams[1].source;

// Only API responses are replaced. The real app, Mermaid runtime and sanitizer
// render the fixture through the sample picker and the Flow workspace.
async function openDiagrams(page: Page, theme: "light" | "dark") {
  await page.emulateMedia({ colorScheme: theme });
  await page.addInitScript((value) => localStorage.setItem("floe.theme", value), theme);
  const responses: Record<string, unknown> = {
    "GET /me": fixture.me,
    "GET /samples": fixture.samples,
    "POST /analyze/sample/qa-rendering": fixture.analyze,
    "GET /analyze/qa-job": fixture.job,
    "GET /llm-config": fixture.llmConfig,
  };
  await page.route(/^https?:\/\/[^/]+\/(me|samples|llm-config|analyze\/(sample\/qa-rendering|qa-job(?:\/stream)?))(?:\?.*)?$/, async (route) => {
    const request = route.request();
    const path = new URL(request.url()).pathname;
    if (path === "/analyze/qa-job/stream") {
      await route.fulfill({ status: 200, contentType: "text/event-stream", body: ": fixture ready\n\n" });
      return;
    }
    const key = `${request.method()} ${path}`;
    if (!(key in responses)) throw new Error(`Unexpected fixture request: ${key}`);
    await route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(responses[key]) });
  });
  await page.goto("/");
  await page.getByRole("button", { name: /QA rendering fixture/ }).click();
  await page.getByRole("button", { name: "QA rendering", exact: true }).click();
  await page.getByRole("button", { name: "Flow", exact: true }).click();
  await expect(page.locator("html")).toHaveClass(theme === "dark" ? /dark/ : /^(?!.*\bdark\b)/);
  await expect(page.getByText("Mermaid parse failed", { exact: false })).toBeVisible();
  await expect(page.locator("pre").filter({ hasText: invalidSource })).toHaveText(invalidSource);
  const diagram = page.locator("figure").filter({ has: page.locator("figcaption", { hasText: "QA valid flowchart" }) });
  await expect(diagram.locator("svg")).toHaveCount(1);
  return diagram;
}

async function renderingState(page: Page) {
  return page.evaluate(() => {
    const root = document.querySelector("#root");
    const outside = [...document.querySelectorAll("body svg, body div[id^='dmermaid-']")]
      .filter((el) => !root?.contains(el));
    const svg = [...document.querySelectorAll("figure")]
      .find((el) => el.querySelector("figcaption")?.textContent === "QA valid flowchart")?.querySelector("svg");
    return {
      theme: document.documentElement.classList.contains("dark") ? "dark" : "light",
      nodes: svg?.querySelectorAll("g.node").length ?? 0,
      edges: svg?.querySelectorAll("path.flowchart-link").length ?? 0,
      labels: [...(svg?.querySelectorAll("text") ?? [])].map((el) => el.textContent?.trim()),
      foreignObjects: svg?.querySelectorAll("foreignObject").length ?? 0,
      orphanSvgs: outside.filter((el) => el.tagName.toLowerCase() === "svg").length,
      orphanMermaidDivs: outside.filter((el) => el.tagName.toLowerCase() === "div").length,
    };
  });
}

for (const theme of ["light", "dark"] as const) {
  test.describe(`Mermaid ${theme}`, () => {
    test.beforeEach(async ({ page }) => {
      const consoleErrors: string[] = [];
      const pageErrors: string[] = [];
      page.on("console", (message) => { if (message.type() === "error") consoleErrors.push(message.text()); });
      page.on("pageerror", (error) => pageErrors.push(error.message));
      // Always retain diagnostics, including when an assertion fails.
      test.info().annotations.push({ type: "fixture", description: "mermaid-rendering.json" });
      browserErrors.set(page, { consoleErrors, pageErrors });
    });

    test.afterEach(async ({ page }, testInfo) => {
      await testInfo.attach("rendering-state", { body: JSON.stringify(await renderingState(page), null, 2), contentType: "application/json" });
      await testInfo.attach("browser-errors", { body: JSON.stringify(browserErrors.get(page)), contentType: "application/json" });
      await testInfo.attach(`mermaid-${theme}`, { body: await page.screenshot({ fullPage: true }), contentType: "image/png" });
    });

    test("retains every node and edge label after SVG sanitization", async ({ page }) => {
      const diagram = await openDiagrams(page, theme);
      await expect(diagram.locator("svg g.node")).toHaveCount(4);
      await expect(diagram.locator("svg path.flowchart-link")).toHaveCount(3);
      // Soft assertions expose every missing label rather than hiding six.
      for (const label of [...nodeLabels, ...edgeLabels]) {
        await expect.soft(diagram.locator("svg text").filter({ hasText: new RegExp(`^${label}$`) }), label).toBeVisible({ timeout: 1_000 });
      }
      await expect(diagram.locator("foreignObject")).toHaveCount(0);
    });

    test("shows invalid source without orphan Mermaid error SVGs", async ({ page }) => {
      await openDiagrams(page, theme);
      // This case is independent of the label assertions above.
      const state = await renderingState(page);
      expect.soft(state.orphanSvgs, "SVGs outside #root").toBe(0);
      expect.soft(state.orphanMermaidDivs, "Mermaid containers outside #root").toBe(0);
      await expect(page.locator("#root .error-icon, #root .error-text")).toHaveCount(0);
      // The application remains usable after a malformed diagram.
      await page.getByRole("button", { name: "Overview", exact: true }).click();
      await page.getByRole("button", { name: "Flow", exact: true }).click();
      await expect(page.locator("pre").filter({ hasText: invalidSource })).toHaveText(invalidSource);
      const reopened = await renderingState(page);
      expect.soft(reopened.orphanSvgs, "SVGs outside #root after reopening").toBe(0);
      expect.soft(reopened.orphanMermaidDivs, "Mermaid containers outside #root after reopening").toBe(0);
    });
  });
}
