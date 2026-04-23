# floe

> **Architectural PR review for TypeScript.** We turn a PR into flows —
> one per architectural story — and tell you three things per flow
> that `git diff` can't: does this flow deliver what the PR claims,
> is there real proof, and how much harder did it just make the code
> to navigate.

## Why flows, not diffs

A modern PR sprawls: N entities, M files, a pile of line changes. Most
of that is noise. Somewhere inside sits the *architectural story* —
"add a retry path," "widen the public API," "split that queue into
two." Reviewers already think in stories; tooling mostly shows them
diffs. floe extracts the stories, then asks three questions of each:

- **Intent-fit** — does this flow actually deliver something the PR's
  stated intent claims? `delivers` / `partial` / `unrelated`, with
  the matching claim cited.
- **Proof** — is there real evidence backing the claim? A benchmark
  log, an example file, a claim-asserting test, a corroborating note.
  **Unit-test presence is not proof.**
- **Nav cost** — signed delta of how much harder the next LLM (or
  human) session has to work to navigate the affected flow.
  Refactors go negative.

The result: a PR of N hunks becomes a PR of K flows, each with a
verdict the reviewer can commit (approve / request-changes / comment)
and export as an agent-ready note bundle.

## Status

Research preview. **TypeScript analysis is end-to-end**:
tree-sitter parse → workspace topology → LSP-backed call graph →
compile pass (tsc) → test/bench runs → LLM synthesis + intent-fit +
proof verification → per-flow cost + token-budget deltas. Rust
analysis starts with Phase A (tree-sitter parse) and will reuse
the same pipeline shape.

Nothing is published to crates.io or npm yet. Run it locally.

## Quickstart

```bash
# Prereqs: Rust stable, Node 20+, Docker (for Postgres), Just
git clone https://github.com/avifenesh/floe
cd floe
cp .env.example .env          # fill in at minimum FLOE_GLM_API_KEY or leave blank
just db-up                    # Postgres in Docker
just dev                      # server + web in parallel
# Open http://localhost:5173, click a sample
```

No Just? `docker compose up -d postgres && cargo run -p floe-server` in one
shell, `cd apps/web && npm install && npm run dev` in another.

## What gets emitted

One JSON artifact per analysis — schema at [`schema.json`](./schema.json).
Highlights:

| Field | Contains |
|---|---|
| `hunks` | Semantic hunks — `call`, `state`, `api`, `lock`, `data`, `docs`, `deletion` |
| `flows` | Entity-clusters with intent-fit + proof verdicts and cost deltas |
| `base` / `head` | Graph snapshots (nodes, call edges, package boundaries) |
| `compile_diagnostics` | `tsc --noEmit` delta, base vs head |
| `test_run` / `bench_run` / `coverage_delta` | External-runner outputs |
| `inline_notes` | Reviewer notes on any object, with rehydrated context for agent export |
| `baseline` | Probe denominators so cost reads as `% of baseline` |

See [`docs/architecture.md`](./docs/architecture.md) for the pipeline
shape and [`docs/hunk-types.md`](./docs/hunk-types.md) for the hunk
vocabulary.

## Layout

```
crates/
  floe-core         Artifact + graph + hunk + evidence types
  floe-parse        TypeScript parse (tree-sitter + workspace topology)
  floe-parse-rust   Rust parse (tree-sitter-rust) — phase A
  floe-hunks        Semantic hunk extractors
  floe-cfg          Per-function control-flow graphs
  floe-flows        Structural flow clustering
  floe-evidence     Claim collection around flows
  floe-cost         Navigation-cost probe pipeline
  floe-probe        Probe runner
  floe-lsp          TypeScript LSP client (async-lsp)
  floe-mcp          MCP-over-stdio host for the synthesis LLM
  floe-server       HTTP server (axum) + Postgres persistence
  floe-cli          One-shot CLI (schema, diff)
apps/web            React + Vite frontend
fixtures/           Sample PRs (base/ + head/ + intent.json + meta.json)
docs/               Architecture, RFC, hunk-type reference
```

## Contributing

See [`CONTRIBUTING.md`](./CONTRIBUTING.md). TL;DR: `just check` before a
PR, bump `PIPELINE_VERSION` only for breaking artifact changes,
regenerate `schema.json` + FE types when you touch `Artifact`.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](./LICENSE-APACHE))
- MIT license ([LICENSE-MIT](./LICENSE-MIT))

at your option. Contributions are dual-licensed under the same terms
unless you state otherwise.
