# Contributing to floe

Thanks for taking the time. floe is a research preview; the goal right
now is to make it **usable for a second reviewer beyond the author**.
Anything that moves toward that is fair game.

## Ground rules

- **One concern per PR.** Adding a hunk type and refactoring flows
  clustering in the same PR is two PRs. Reviewers (including us)
  won't see the forest.
- **Back claims with real data.** If you're improving a heuristic,
  show a before/after run on `fixtures/pr-0002-state-widen` (or any
  other fixture). Paste the numbers in the PR body.
- **Don't introduce a name unless it earns it.** A chip, a button,
  a status — if the codebase already has a vocabulary for it,
  reuse the name. Divergent names fragment reviewer mental models.

## Quickstart for contributors

```bash
git clone https://github.com/avifenesh/floe
cd floe
cp .env.example .env

# Postgres in Docker
just db-up

# Run server + web in parallel
just dev
```

If you don't have `just`, the [justfile](./justfile) lists every
recipe plain-text.

## Pre-PR checklist

Run this:

```bash
just check
```

That runs, in order:

1. `cargo test --workspace --no-fail-fast`
2. `cargo clippy --all-targets -- -D warnings`
3. `npx tsc --noEmit` in `apps/web`
4. `npm run build` in `apps/web`

If you touched `Artifact` in `crates/floe-core/src/artifact.rs`:

```bash
just regen-schema
```

That regenerates `schema.json` and `apps/web/src/types/artifact.ts`.
CI fails if either drifts.

## When to bump `PIPELINE_VERSION`

`PIPELINE_VERSION` (in `crates/floe-server/src/cache.rs`) is mixed
into every cache key. Bumping invalidates every prior artifact on
every dev's disk.

**Bump only for breaking changes:** a field's shape or semantics
change in a way old JSON can't represent, or a pipeline stage's
output changes meaning.

**Don't bump for additive changes:** new optional fields with
`#[serde(default)]`, new enum variants not present in prior JSON,
new passes that attach fresh data. Old artifacts deserialize fine;
blunt invalidation just discards expensive LLM work.

## Adding a hunk type

1. Add the variant to `floe_core::hunks::HunkKind` (additive, no
   `PIPELINE_VERSION` bump needed).
2. Add an extractor in `crates/floe-hunks/src/<kind>.rs` and expose
   it from `lib.rs`.
3. Wire into `floe-server/src/worker.rs` alongside the other
   extractors.
4. Add exhaustive match arms in `floe-flows`, `floe-evidence`,
   `floe-mcp` (wire, handlers, state), `floe-server/src/llm/*`.
5. Add a frontend branch to `apps/web/src/views/pr/PrHunks.tsx`.
6. Add a fixture in `fixtures/pr-<NNNN>-<kind>/` that exercises it.
7. `just regen-schema`.

## Adding a pass

A pass enriches the artifact after the structural pipeline lands.
Conventions:

- Live under `crates/floe-server/src/passes/`.
- Expose an async `attach(&mut artifact, ...)` that mutates in place.
- Gate on an env knob (`FLOE_<NAME>_PASS=1` or equivalent).
- Either run synchronously before `READY` (fast, graph-only) or
  spawn from `worker.rs` and merge via `merge_into_artifact` (slow,
  background).
- Degrade silently: if the pass can't run, skip and optionally push
  a `notice` so the reviewer sees why downstream axes are empty.

## Code style

- **Rust:** `cargo clippy --all-targets -- -D warnings` is the bar.
  Prefer early-return for clarity; avoid panics outside tests.
- **TypeScript:** `npx tsc --noEmit` clean. Tailwind for styling;
  theme tokens only (`bg-muted/60`, `border-border/60`) so light
  mode stays consistent.
- **Comments:** only when a *why* would surprise a future reader —
  a hidden invariant, a workaround, a bug we already hit.

## License

By contributing, you agree your contributions will be licensed under
the project's dual MIT / Apache-2.0 license.
