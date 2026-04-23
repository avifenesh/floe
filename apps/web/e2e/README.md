# Playwright e2e

One happy-path smoke test: anon visitor → picks a sample → waits for
pipeline READY → opens a flow workspace.

## Run

```
cd apps/web
npm i -D @playwright/test
npx playwright install chromium
npx playwright test
```

`playwright.config.ts` auto-spawns the Rust server + Vite dev server
locally. In CI, set `CI=true` and have the server already running.

## Add a test

Put new specs in `e2e/*.spec.ts`. Keep them stateless — no auth,
no DB mutations — so CI can run them against a shared deployment
without cleanup.
