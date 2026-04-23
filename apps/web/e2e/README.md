# Playwright e2e

One happy-path smoke test: anon visitor → picks a sample → waits for
pipeline READY → opens a flow workspace.

## Run locally

```
cd apps/web
npm ci                  # pulls @playwright/test from devDependencies
npm run e2e:install     # downloads chromium once
npm run e2e
```

`playwright.config.ts` auto-spawns the Rust server + Vite dev server.
Postgres needs to be up first (`just db-up` from the repo root).

Not in required CI yet — the e2e job was getting flaky with the
postgres + cargo + Vite triplet. Fix + re-enable is tracked as a
follow-up.

## Add a test

Put new specs in `e2e/*.spec.ts`. Keep them stateless — no auth,
no DB mutations — so CI can run them against a shared deployment
without cleanup.
