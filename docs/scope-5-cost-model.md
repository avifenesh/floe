# Scope 5 · cost model — probe-based design

Status: **draft** · 2026-04-18 · supersedes the heuristic `adr-cost` v0 stub.

## Why the v0 stub is wrong

The shipped `adr-cost` scores a flow as `api×3 + call×2 + state×2 + files + no-tests + entity-breadth`. That measures "how much diff did this flow touch" which is backwards for refactors: a PR that simplifies the repo should make cost go **down**, not up. The RFC §9 principle is literally *"how expensive is it for the next session to safely continue work on the affected flow?"* — a navigation-cost question, not a diff-size question.

Conclusion: rip out the heuristic, replace with probe-based measurement.

## Principle

Cost is the **delta** between two measurements of how expensive it is for a pinned LLM to answer a fixed set of questions about the repo, pre-PR vs post-PR. Negative delta = the PR made the repo easier to navigate. Positive delta = harder.

Measurement unit is observed LLM effort while answering, not the answer's correctness. Correctness (claim vs implementation) is a separate validation pass, explicitly deferred.

## Probe set (v0, frozen)

Three questions. Each runs in its own clean session so token counts per question stay comparable.

| ID | Question | Primary axis |
|---|---|---|
| `probe-api-surface` | *"Map the public API of this repo. For every exported function, class, or method, give a one-line description of its functionality."* | continuation + docs_alignment |
| `probe-external-boundaries` | *"List external boundaries: network calls, filesystem writes, subprocess spawns, DB queries, and their trust classification."* | operational |
| `probe-type-callsites` | *"For each exported type, list every call-site that constructs, extends, or destructures it."* | retrieval_ambiguity + runtime |

Each question is paired with the same tool surface: `adr.get_entity`, `adr.neighbors`, plus PI-style `read` / `grep` / `glob` over the repo root.

## Model pin

Probe model is **pinned** per baseline. Default `qwen3.5:27b-q4_K_M` (local, free to re-run, stable as long as Ollama holds the tag). Override via `ADR_PROBE_MODEL`. Synthesis model is separate — changing the synthesis model does **not** invalidate baselines.

Signature for baseline key: `(repo_key, sha, probe_model, probe_set_version)`.

## Measurement

For each probe session, we record:

- `tokens_in` / `tokens_out` (from the chat response)
- `turn_count`
- `tool_call_count` — total and per-tool-name
- `per_entity_visits` — for every `adr.get_entity`, `adr.neighbors`, and `read(file containing entity)` call, tally which qualified names appeared in scope

Per-entity cost observation:

```
cost[entity] = α · visits[entity]
             + β · tokens_spent_while_entity_in_context
             + γ · turns_where_entity_appeared
```

v0 weights: `α=1.0`, `β=0.001`, `γ=2.0`. Frozen at probe-set v0.1; any change bumps `probe_set_version` and invalidates baselines.

## Storage

Local (self-hosted tester path):

```
.adr/baseline/
  <repo_key>/                       # hash of normalized repo root path
    <sha>/
      <probe_model>/
        probe-api-surface.json
        probe-external-boundaries.json
        probe-type-callsites.json
        aggregate.json              # per-entity sum across the three probes + metadata
```

Hosted path (v1+, sketch): same schema, S3 backing, `s3://adr-baselines/<repo_key>/<sha>/<probe_model>/...`.

`aggregate.json` schema:

```jsonc
{
  "schema_version": "0.1.0",
  "repo_key": "<blake3 of root path>",
  "sha": "abcd…",
  "probe_model": "qwen3.5:27b-q4_K_M",
  "probe_set_version": "0.1",
  "computed_at": "2026-04-18T13:00:00Z",
  "duration_s": 187,
  "per_entity": {
    "Queue.setBudget": { "cost": 42.3, "visits": 7, "tokens": 3820, "turns": 3 },
    // …
  },
  "totals": { "entities": 124, "tokens": 52000, "tool_calls": 89, "turns": 28 }
}
```

## Invalidation

A baseline is stale if any of:

- `sha` mismatch vs the commit being analyzed
- `probe_model` differs from the env-pinned model
- `probe_set_version` differs
- `computed_at` older than `ADR_BASELINE_TTL_DAYS` (default 60)

When stale, the pipeline re-runs the probe.

## Pipeline — async

Worker stages with probe:

```
parse → cfg → hunks → structural-flows → llm-synthesis → evidence → READY(structural cost=null)
                                                                           ↓
                                                      (async) probe-base → probe-head → cost-attribute → UPDATE
```

1. After evidence lands, the artifact is written to the cache with `cost_status: "analyzing"` and the client sees `status: "ready"`. Flow/Overview/Source/Diff all render immediately.
2. A background task spawns:
   - Ensure base-baseline exists (run probe on base if missing / invalid).
   - Ensure head-baseline exists (run probe on head).
   - Compute per-entity `delta = head[e] - base[e]`.
   - Fold delta into per-flow `Cost { net, drivers, axes }`.
   - Mutate the cached artifact in place; broadcast SSE `{ stage: "cost", percent: 100, flows: <n> }`.
3. Frontend Cost sub-tab: shows "Analyzing…" while `cost_status == "analyzing"`; renders the signed scores once `cost_status == "ready"`.

## Cost shape (schema update)

Drop the current `Cost { net: u32, drivers: Vec<CostDriver> }`. Replace with:

```rust
pub struct Cost {
    pub net: i32,              // signed; − means refactor improved navigation
    pub axes: Axes,            // signed per-axis
    pub drivers: Vec<CostDriver>,
    pub probe_model: String,
    pub probe_set_version: String,
}

pub struct Axes {
    pub continuation: i32,
    pub runtime: i32,
    pub operational: i32,
    pub proof: i32,            // v0: always 0; filled by evidence-PROOF collectors in later scope
}
```

`cost_status: "analyzing" | "ready" | "skipped" | "errored"` at `artifact.cost_status` so the UI can branch cleanly.

## UI

- **Cost sub-tab** while `analyzing`: monochrome spinner + short log ("probing base sha abc1234d · 37s elapsed"). Other sub-tabs unaffected.
- **Cost sub-tab** when `ready`: big net number (signed; negative rendered as `−12` not `-12`), four axis scores on a single-row sparkline, driver list below with per-driver sign.
- **Flow list scale strip**: when all flows have cost, show `flow-budget: +30 · streaming: -4 · suspend: +3` line.

## Out of scope (deferred)

- Correctness validation of probe answers — separate pass, scope 6+.
- Per-PR runtime LLM cognition probe (§11 defers this; the repo-level probe is the v0 path).
- S3 / hosted baseline backing — v0 is filesystem-only.
- Multi-model probe ensembles — v0 pins one.
- Streaming probe with partial updates — v0 is atomic per question.

## Milestones

1. `adr-probe` crate scaffolding: probe definitions, agent loop reusing existing `llm::ollama_client` + `adr-mcp` tool surface, per-entity tallying.
2. Baseline storage module (filesystem; read/write/invalidate).
3. Worker integration: post-evidence hook, async task, SSE progress.
4. Cost aggregation module: delta math + axis rollup.
5. Frontend `cost_status` branching + signed rendering.
6. Smoke on glide-mq #181: first PR bootstraps baseline; second PR reuses it.

Each is a discrete delivery; stop between milestones to eval.
