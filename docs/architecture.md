# Architecture

floe is a pipeline that turns a PR (base + head snapshots) into a
single JSON artifact describing the architectural deltas, plus a web
UI for reviewing it. This doc sketches the shape of the pipeline and
the invariants that hold across passes.

For the historical design rationale, see [`rfc-v0.3.md`](./rfc-v0.3.md).

## Pipeline shape

```
┌──────────┐   ┌───────────┐   ┌─────────┐   ┌─────────┐
│ parse    │─▶ │ topology  │─▶ │ cfg     │─▶ │ hunks   │
└──────────┘   └───────────┘   └─────────┘   └─────────┘
                                                 │
                                                 ▼
┌──────────┐   ┌────────────────────────────────────────┐
│ evidence │◀─ │ flows (structural clustering)          │
└──────────┘   └────────────────────────────────────────┘
     │
     ▼                         publish READY
┌────────────────────────────────────────────────────────┐
│ claim-anchoring (sync, declaration sites only)         │
└────────────────────────────────────────────────────────┘
     │  ... artifact goes live; background passes continue ...
     ├─▶ compile delta  (tsc --noEmit on base + head)
     ├─▶ external runs  (FLOE_TEST_CMD, FLOE_BENCH_CMD)
     ├─▶ coverage delta (lcov / vitest json-summary)
     ├─▶ claim-anchor fan-out (LSP references, head + base)
     ├─▶ intent extraction   (Qwen local, when no intent supplied)
     ├─▶ probe pass          (cost baselines, per-flow cost)
     ├─▶ synthesis pass      (LLM flow naming)
     └─▶ intent-fit + proof  (GLM cloud, per-flow verdicts)
```

Each background pass writes back via `merge_into_artifact` under the
artifact lock so later passes see earlier results.

## Crate layout

| Crate | Responsibility |
|---|---|
| `floe-core` | Artifact, graph, hunk, evidence, provenance types. No IO. |
| `floe-parse` | TypeScript parse + workspace topology via tree-sitter. |
| `floe-parse-rust` | Rust parse (Phase A only so far). |
| `floe-lsp` | TypeScript language server client, `async-lsp`-based. |
| `floe-cfg` | Per-function control-flow graphs. |
| `floe-hunks` | Semantic hunk extractors (call/state/api/lock/data/docs/deletion). |
| `floe-flows` | Structural flow clustering from hunks + graph. |
| `floe-evidence` | Claim collection attached to flows. |
| `floe-cost` | Cost-probe baselines + per-flow signed deltas. |
| `floe-probe` | Probe runner (drives the local LLM probe models). |
| `floe-mcp` | MCP-over-stdio host for synthesis-pass LLMs. |
| `floe-server` | HTTP surface (axum), Postgres persistence, pipeline orchestrator, background passes. |
| `floe-cli` | One-shot `diff` + `schema` utilities. |

## Artifact contract

The pipeline's only output is `Artifact` (see
[`schema.json`](../schema.json)). Three rules:

1. **Additive evolution wins.** New optional fields with
   `#[serde(default)]`, new `HunkKind` / `ClaimKind` variants.
   Never break old JSON unless you bump `PIPELINE_VERSION` — and
   even then, prefer a compatibility shim on load.
2. **Every claim carries provenance.** `provenance.source`,
   `version`, `pass_id`. If a claim can't be traced back to what
   produced it, the reviewer can't trust it.
3. **`SourceRef`s are 1-indexed and UTF-16-column.** Normalised
   on the Rust side so the UI never re-normalises.

## Persistence

Postgres stores:

- `users` — OAuth identity.
- `sessions` — signed cookie sessions.
- `pr_analyses` — `(user, repo, pr_number, head_sha, intent_fp,
  llm_sig) → artifact_key`. The artifact itself lives on disk in
  the cache under `artifact_key.json` (JSON is large; Postgres is
  for queries, not blobs).
- `inline_notes` — per-object reviewer notes keyed on `(jobId,
  anchor)` so they survive re-runs of the same head.

SQLite is supported as a fallback (`FLOE_DB=sqlite`) for single-user
dev; every query runs through `db.rs` with both backends wired.

## Cache invalidation

Cache key = blake3 of `(PIPELINE_VERSION | SCHEMA_VERSION | head_sha
| llm_signature | intent_fingerprint)`. Two runs with identical
inputs and identical LLM regime hit the same entry; changing any
axis invalidates that entry but leaves siblings untouched.

`PIPELINE_VERSION` bumps **everything** on disk — use sparingly.
See [`CONTRIBUTING.md`](../CONTRIBUTING.md#when-to-bump-pipeline_version).

## LLM regime

Three models are configured independently:

- **Probe** (local small model) — produces cost baselines. Must be
  deterministic-enough to compare base vs head.
- **Synthesis** (GLM cloud default) — flow naming via MCP tool calls.
  Host validates every proposed mutation; model never writes the
  artifact directly.
- **Proof / Intent-fit** (GLM cloud default) — prose analysis of
  claims vs code. Needs strong reasoning; local small models
  hallucinate here.

Each stamps its model + version on the artifact so the drift banner
can tell the reviewer when their cached numbers no longer match the
live regime.

## Not in scope (for the preview)

- Focus mode, narrative export (GIF/MP4), agent-packet projection,
  TLA+/Apalache verification gate, GitHub-App distribution — all
  deferred per RFC §11.
- Python / Go / Java pipelines. Rust is next after TS.
