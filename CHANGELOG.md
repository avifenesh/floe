# Changelog

All notable changes to floe are recorded here. Format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/). Versioning
will switch to SemVer once the artifact schema stabilises; today,
the schema is pinned by `SCHEMA_VERSION` + `PIPELINE_VERSION` and
described by [`schema.json`](./schema.json).

## [Unreleased]

First public push. What the preview ships:

### Pipeline

- TypeScript end-to-end: tree-sitter parse → workspace topology
  (pnpm / npm / yarn / tsconfig references) → LSP-backed call graph
  (`typescript-language-server`) → hunk extraction → structural flow
  clustering → evidence collection → cost-probe baselines → LLM
  synthesis (flow naming) → intent-fit + proof verification →
  compile-unit delta (`tsc --noEmit`) → test / bench runs →
  coverage delta (lcov + vitest json-summary) → claim anchoring
  with LSP `references` fan-out.
- Rust phase A: tree-sitter parse of `*.rs` files into `Function` /
  `Type` / `Enum` / `Trait` graph nodes. Call graph + compile pass
  + test runs land in phases B–D.

### Hunk vocabulary

`call` · `state` · `api` · `lock` · `data` · `docs` · `deletion`.

- **lock** — sync primitives (TS: `async-mutex`, `p-limit`, `p-queue`,
  `async-lock`, `Atomics`; Rust: `Mutex`, `RwLock`, `Atomic*`,
  `OnceCell`, `OnceLock`).
- **data** — serde-serializable / `interface` / `z.object` /
  Rust `struct` field-set diffs with single-rename heuristic.
- **docs** — JSDoc `@param` drift vs function signature.
- **deletion** — base-only entities absent from head's graph.

### UI

- Anonymous landing with sample gallery + 3-card product pitch.
- Authenticated dashboard as a chronological feed (stats strip,
  resume chip, feed cards, compare-2 selection).
- PR workspace with two-row top spine (PR + per-flow tabs), flow
  filter/search, source diff search, 3-step onboarding tour,
  responsive flow graph (stacked list below `md`).
- Inline notes on every reviewable object (flow, entity, claim,
  hunk, file-line) with agent-export bundle.
- Baseline-drift banner with "rerun now" action; suppresses axes
  where the cached artifact predates the model-stamping fix.

### Infrastructure

- Postgres persistence for PR analyses, verdicts, inline notes.
- MCP host (`floe-mcp`) spoken over stdio JSON-RPC 2.0; synthesis
  LLMs run as MCP clients.
- GLM-4.7 cloud primary, Qwen 3.5 27B local fallback; rate-limit
  handling via semaphore + circuit breaker.
- Cache key mixes `SCHEMA_VERSION`, `PIPELINE_VERSION`, head
  snapshot sha, LLM signature, intent fingerprint.
- CI on GitHub Actions: `cargo test` + `cargo clippy -D warnings`
  + `npx tsc --noEmit` + `npm run build` + schema-drift guard
  (Postgres service container).
- Playwright happy-path e2e (anon → sample → flow opens).

### Known gaps

- Rust pipeline phases B–D (compile pass, external runs, call graph
  via rust-analyzer) not yet wired.
- `unsafe` as a first-class hunk kind is planned but not shipped.
- Samples don't yet exercise every hunk type end-to-end; fixtures
  for lock / data / docs / deletion exist but haven't been run
  through the full pipeline.
- No GitHub App — URL-driven analysis requires a signed-in session.
