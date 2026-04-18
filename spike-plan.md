---
title: "Spike plan · v0 · architecture delta review"
version: 0.2
date: 2026-04-18
status: Active
context: Solo developer · self-hostable product · testers-only rollout · no team, no meetings
companion: architecture_delta_review_rfc.md (v0.2)
timeline: 10–12 weeks
prior_version: v0.1 (2026-04-17) — preserved in git history
---

# Spike plan · v0 · v0.2 (flow-first pivot)

Six scopes. Each ends with a demoable acceptance gate; failing a gate is a signal to stop or narrow, not to extend the scope.

## What changed in v0.2

- **Product unit is a flow, not a PR.** Every scope past 2 operates per-flow primary.
- **LLM is a hot-path classifier**, not a calibration-time probe. It runs inside PI (Ollama's minimal coding agent) via an `adr` PI extension we ship. **Gemma 4 26B MoE is the product target**, with stronger models as the ceiling. Small (<10B) models were considered and dropped after smoke tests — our audience ships on capable hardware, and "works on everything" is not a promise worth making.
- **Scope 3 is rebuilt around flow synthesis** (was "seven views come alive"). Per-flow rendering is now scope 4.
- **Scope 5 merges evidence + cost + baseline pinning** — the work that used to fill scope 4 compresses because the signing-delta surface is now per-flow and thinner per slice.
- Eval (scope 6) tests the LLM-assisted surface against the deterministic-only surface in addition to raw diff.

## Solo-developer posture

Unchanged from v0.1. Rubrics instead of pair review; external loom checkpoints at scope 3, 4, 5; 2-week hard timebox per scope; pre-committed kill criteria.

## At-a-glance

| # | Scope | Weeks | Acceptance demo |
|:---|:---|:---|:---|
| 1 | ✅ Graph + schema + hunks (call · state · api, method-level) | 1–2 | `adr diff` on glide-mq PR #181 emits 12 hunks with full method signatures |
| 2 | ✅ CFG + orchestrator + frontend scaffold + PR view + Source view with syntax highlighting + architectural overlay | 3–4 | Real glide-mq PR renders PR + Source views end-to-end |
| 3 | **Flow detection: deterministic clustering + PI extension + Ollama + Gemma 4 runner** | 5–6 | `adr diff` produces an `artifact.flows[]` for glide-mq PR #181 with both structural and LLM-assisted paths; banner when falling back |
| 4 | **Flow-first frontend: scope ribbon + scoped renderers for PR/Source + Flow/Morph/Delta/Evidence stubs** | 7–8 | Reviewer can scope the UI to any detected flow; PR view = flows overview; other views render per-flow and cross-flow modes |
| 5 | Evidence + per-flow cost v2.3 + baseline pinning + calibration report | 9–10 | Held-out glide-mq + Inngest PRs render per-flow cost + evidence; `calibration-report.md` committed |
| 6 | Real eval with 8 known-figure reviewers (A/B: flow-first vs raw-diff · LLM-assist vs structural-only) | 11–12 | `eval-report.md` committed with experience + time as primary metrics |
| 7 *(was 6)* | Narrow or kill | — | One of {kill memo · narrowed scope · v1 RFC} signed and committed |

Timeline stays 10–12 weeks; scopes compressed at 5 and 7 to hold the budget.

## Cross-cutting

- **Cargo workspace**: `adr-core` · `adr-parse` · `adr-hunks` · `adr-cfg` · `adr-flows` *(new)* · `adr-evidence` · `adr-cost` · `adr-mcp` *(renamed purpose: Rust-side PI runner + validation host for the extension)* · `adr-eval` · `adr-server` · `adr-cli`.
- **PI extension**: npm package `@adr/pi-extension` — Node.js, TypeScript. Installed via `pi install npm:@adr/pi-extension` on the tester's machine or shipped alongside the Rust binary as a bundled asset.
- **Frontend**: Vite + React + Tailwind + shadcn/ui in `apps/web`.
- **Types**: Rust → TS via `ts-rs`.
- **Testing**: `cargo test` + `insta` snapshots on JSON artifacts; fixture PRs as golden inputs.
- **Kill gates** (RFC v0.2): experience ≥ 4/5 median · time −25 % on target classes · seeded-bug catch-rate strictly higher than raw diff · LLM-assist ≥ structural-only on at least one metric with significance.

---

## Scope 1 · graph + schema + hunks (weeks 1–2) · ✅ done

**delivered**
- Cargo workspace with `adr-core` · `adr-parse` · `adr-hunks` · `adr-cfg` · `adr-server` · `adr-cli`.
- Graph schema v0.1: versioned, JSON-serializable, full provenance on every node + edge.
- Three hunk extractors (call · state · api), method-level granularity, `this.*()` resolution, multi-line signature capture.
- Fixture corpus: pr-0001 through pr-0006 covering call/state/api/combined/noop/cross-file.
- `insta` snapshot tests on graphs + hunks + end-to-end.
- Proven on glide-mq PR #181 (2.8 K/1.6 K, 39 files) → 12 real hunks extracted.

---

## Scope 2 · orchestrator + frontend scaffold + first views (weeks 3–4) · ✅ done

**delivered**
- `adr-cfg`: per-function CFG with entry · exit · seq · branch · loop · async-boundary · throw · try · return.
- `adr-server`: axum + tokio + fs cache. `POST /analyze`, `GET /analyze/:id`, SSE stream, file-serving endpoint.
- `apps/web`: Vite + React + Tailwind v3 + shadcn-compatible CSS variables + dark-mode toggle via theme context + typography locks (Inter + JetBrains Mono, no ligatures, tabular nums).
- PR view: header + stats + architectural-delta hunks list with word-level tint for paired rows and full-strength tint for pure add/remove.
- Source view: IDE-style file tabs + unified diff with red/green backgrounds + word-level segments + shiki syntax highlighting + collapsed-context skip blocks with click-to-expand + architectural overlay strip with hover-revealed chip.

---

## Scope 3 · flow detection (weeks 5–6) · *new — this is where the pivot lives*

**goal** · convert an artifact's flat hunk list into `artifact.flows[]` — the primary unit everything downstream will operate on. Two code paths: deterministic clustering that always runs, LLM-assisted synthesis via PI + Ollama + Gemma 4 that runs when configured and validates via host-enforced invariants.

**deliverables**

1. **`adr-flows` crate**: deterministic clustering.
   - Hybrid: call-graph connected components + type-propagation groupings.
   - Outputs `Flow { id, name: "<structural>", rationale, entities, hunk_ids, confidence: structural }`.
   - Invariant: every hunk appears in ≥ 1 flow (fallback "misc" flow catches orphans).
   - Hunks may appear in multiple flows — explicit.

2. **`@adr/pi-extension`** — npm-installable PI extension, TypeScript.
   - Read tools: `list_hunks`, `get_entity(id)`, `neighbors(id, hops)`, `list_flows_initial()`.
   - Mutation tools: `propose_flow`, `mutate_flow`, `remove_flow`, `finalize`.
   - Talks to a local socket/pipe exposed by the Rust `adr-mcp` host per analysis run.
   - Installable via `pi install npm:@adr/pi-extension` or bundled path.

3. **`adr-mcp` crate** (Rust) — the validation host.
   - Spins up a local socket per analysis; attaches it to an artifact in memory.
   - Validates every mutation tool call against graph invariants.
   - Final acceptance gate: every hunk covered ≥ 1×, all entity IDs exist, no generic bucket names, tool-call rate under cap.
   - Rejects and falls back to deterministic on any invariant violation.
   - Tees progress events to the `adr-server` SSE stream so the frontend sees "detecting flows…" live.

4. **Rust-side PI runner** in `adr-server` (or in `adr-mcp`).
   - Invokes `ollama launch pi --model gemma4:26b-a4b-it-q4_K_M -- --extension @adr/pi-extension --artifact <path>` (exact flags pinned after smoke test).
   - Supervises PI process, captures stdout/stderr, enforces timeout (5 min default), reports back.
   - Flow-by-flow iterative mode for Gemma 4 E4B: drive one cluster at a time.

5. **Config**: `.adr/llm.toml` extended.
   ```toml
   [flow_synthesis]
   runner = "ollama+pi"       # or "off" to force structural
   model  = "gemma4:26b-a4b-it-q4_K_M"
   mode   = "single-pass"     # or "per-flow" for small models
   ```

6. **Frontend placeholder** for flow list.
   - No new view yet (scope 4). On PR view, show a terse list of detected flows with `{ name, hunk_count, source }` and a banner when `source === "structural"`.

**decisions locked inside scope**

- Extension language: **TypeScript** — PI is Node-based and this keeps install friction to one `pi install …`.
- Extension protocol: **local socket over a per-run path** (Unix domain socket on POSIX, named pipe on Windows). Avoids HTTP noise in logs and is easy to scope to one job.
- Rejection policy: **whole-run rejection**. We don't half-accept LLM output. If `finalize()` fails any invariant, the whole LLM pass is discarded and we ship deterministic flows with a banner.
- LLM timeout: **5 min soft, 10 min hard**. Beyond hard, kill PI and fall back.
- Calibration repo for scope-3 tuning: **glide-mq** first (real vibe-coded multi-flow PRs). Inngest left for scope 5.
- Prompt + tool surface: **iterate in scope 3, freeze at end of week 6**. Any prompt change after freeze requires re-running the eval — treat as ABI.

**acceptance**

- `adr diff` on glide-mq PR #181 produces both:
  - structural flows (always) with explicit "structural" label,
  - LLM-assisted flows (Gemma 4 26B MoE running through PI + the extension) with `source: "llm:gemma4-26b-a4b@<version>"` and all invariants passing.
- `adr diff` on the same PR with Gemma 4 E4B in flow-by-flow mode also completes without crash; output labeled and stored.
- When `flow_synthesis.runner = "off"`, the artifact ships structural flows with a visible banner on PR view.
- Rejection path exercised: manually corrupt the extension to propose a fake entity; host rejects the run; fallback ships clean.

**risks**

- **PI's API surface changes**. Mitigation: thin extension, treat PI protocol as ABI, maintain an `adr-pi-extension` version matrix.
- **Gemma 4 E4B tool-calling reliability too low**. If E4B rejects > 50 % of runs, we narrow the floor to "26B MoE only" and document the hardware requirement honestly.
- **Prompt engineering rabbit hole**. Mitigation: freeze prompt at end of week 6, don't touch it during scope 4.
- **Extension install friction**. Mitigation: ship the extension alongside the Rust binary; `adr init` installs it.

**out-edges** · `artifact.flows[]` stable for scope 4 · PI-runner abstraction reusable for scope-5 calibration probes · rejection banner already surfaced in UI.

---

## Scope 4 · flow-first frontend (weeks 7–8) · *new*

**goal** · every view knows what a flow is. The reviewer can pick a flow scope and see every panel of the product re-focus. PR view becomes the flows overview.

**deliverables**

1. **Spine scope ribbon**: `[all flows]` + one chip per flow · click = scope that view · keyboard navigation.
2. **PR view rewrite**: flows overview (cards: name · rationale · entity count · hunk count · source badge · confidence) · "map" toggle for cross-flow entity sharing · deep link to a flow.
3. **Source view scoped rendering**: when a flow is selected, the file tabs reorder so flow-participating files come first; the diff shows only hunks in that flow plus their propagation context; architectural overlay chips show the flow name.
4. **Flow view** (first real pass): runtime trajectory visualization for the selected flow — side-by-side canvases for base and head, packet animation along changed edges.
5. **Morph / Delta / Evidence / Cost**: stub pages that honour scope (render the entities / hunks filtered to the current flow, even if the view itself is not fully designed).
6. **State persistence**: selected flow persists across view switches and survives page reload (URL hash).
7. **Banner component**: when `artifact.flows.some(f => f.source === "structural")`, a persistent banner reads *"Flows detected structurally — LLM synthesis not available."*

**decisions locked inside scope**

- Flow ribbon is part of the spine, not a separate row — it shares the 40 px band and wraps on narrow viewports.
- URL hash format: `#f=<flow-id>` (stable across sessions). Reload restores.
- No per-flow theming yet (all flows use the same palette). Per-kind accent for Call/State/API hunks is still deferred.
- Cross-flow "map" mode uses a simple force-directed SVG; prettier canvas comes in v1 if justified.

**acceptance**

- glide-mq PR #181 loaded: reviewer sees 3–5 flows on the PR view. Clicking "budget redesign" scopes every other view to the 6–8 budget-related hunks. Clicking "all flows" restores.
- Source view tab order changes based on scope; diff filter works.
- Flow view shows a per-flow runtime trajectory, even if the data is coarse.
- URL hash round-trips across reload.

**risks**

- **Over-designing Flow / Morph / Delta at once**. Mitigation: cost + evidence stay stubs this scope; real visuals come in scope 5.
- **Flow count outruns ribbon space**. Mitigation: overflow menu at ribbon end.

**out-edges** · flow-scoped views stable for scope 5 to add per-flow cost/evidence · deep-link primitive reused by eval harness.

---

## Scope 5 · evidence + per-flow cost + baseline (weeks 9–10)

*(compressed from former scopes 4 + 5)*

**goal** · every flow carries real evidence references and a signed cost. Baselines are pinned including the flow-synthesis model. Calibration report is in the repo.

**deliverables**

1. `adr-evidence` · four collectors (PERF · DATA · API · LOCK). Each evidence unit attaches to a flow, not the PR.
2. `adr-cost` · cost v2.3 per flow. Drivers first, net gated by confidence ≥ 0.70. Aggregated PR net is the sum.
3. `adr baseline` · pinned `(commit_sha, llm_tool, llm_model, llm_version, flow_synthesis_model)`. Mismatch refuses the delta and surfaces "re-baseline required".
4. `adr-llm` · adapters for ceiling-check targets (claude · codex · gemini · opencode). Unchanged from v0.1.
5. **Calibration set**: 24 PRs split across glide-mq (15) + Inngest (9) with hand-labeled `expected.toml` per PR. Solo-labeled with written rubric + one-week re-label pass.
6. `adr calibrate` · fits linear cost coefficients; emits `coefficients.toml` + `calibration-report.md`.

**acceptance** · per-flow cost strip renders on Cost view with real coefficients · baseline refuses comparison when any pinned field drifts · `calibration-report.md` committed.

**risks** · thin bench coverage on glide-mq · solo labeling subjectivity — both addressed in v0.1 plan, same mitigations (document strength: none; written rubric; re-label pass).

---

## Scope 6 · real eval (weeks 11–12)

*(merged from former scope 5)*

**goal** · produce an honest answer to two questions:

1. For which TS PR classes does the flow-first surface beat raw-diff review?
2. Does the LLM-assisted flow detection matter vs structural-only, on the floor model?

**deliverables** (unchanged from v0.1 plus the LLM-assist axis)

- 10 held-out PRs (5 glide-mq + 5 Inngest) + 3 seeded architectural bugs.
- 8 known-figure reviewers at $100/2 PRs ($300 if few-K lines). Recruitment started in scope 3, confirmed end of scope 4.
- `adr-eval` crate: serves precomputed artifacts, review form, stores `eval-run-<ts>/*.jsonl` tamper-resistantly.
- Metrics: *primary* experience Likert + time; *secondary* correctness including seeded bugs and LLM-assist-vs-structural A/B.
- Expert panel (2 senior TS reviewers) producing ground truth.

**acceptance**

1. 13 PRs × 16 reviews + ground truth; no dropouts.
2. `eval-report.md` committed with experience, time, correctness.
3. Seeded bugs: raw diff ≤ 1/3, v0 surface ≥ 2/3.
4. Median Likert ≥ 4/5.
5. LLM-assist vs structural-only: at least one metric shows significant difference on the floor model (E4B).

---

## Scope 7 · narrow or kill (spillover, week 12 end)

*(was scope 6 in v0.1)*

Unchanged: read eval report cold · written decision · one of {kill · narrow · v1 RFC} committed · public write-up · debrief reviewers · codebase disposition.

**Kill trigger updated**: if correctness passes, experience is flat, *and* LLM-assist provides no measurable benefit over structural at the floor model — kill. We sold "LLM-assisted architectural cognition on a $0 floor"; if neither half of that delivers, the product isn't.

---

## Addenda

- **Rubric-first labeling** · unchanged.
- **External checkpoints** · end of scope 3 (flow detection working), 4 (flow-first UI working), 5 (calibration done). 15-min loom each.
- **Timeboxing** · 2 weeks hard. Missing work gets deleted.
- **Public commit cadence** · weekly.
- **Decision pre-commits** · kill criteria in RFC v0.2, re-signed at scope 3 end when we've seen real LLM output.
