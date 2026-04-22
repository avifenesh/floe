---
title: "RFC: Architecture delta review · v0 spike"
version: 0.3
date: 2026-04-22
status: Decided · in-build (scope 5 continuation)
supersedes: architecture_delta_review_pre_rfc.md
prior_version: v0.2 (2026-04-18) — preserved in git history; see Appendix D for the v0.2→v0.3 delta. v0.1 delta lives in Appendix C.
first_analyzed_language: TypeScript
backend_runtime: Rust (tokio)
frontend: Vite + React + Tailwind + shadcn/ui
primary_substrate: Self-hostable web review app (testers only in v0)
primary_unit_of_review: Flow (not PR) — a PR contains 1..N flows
calibration_repos: glide-mq (primary), Inngest (secondary)
proposal_sheet_format: Explicit YAML block in PR description
cost_model_principle: Per-flow deterministic drivers grounded in LLM-navigation research; LLM participates at runtime via MCP to group & validate flows (classifier, not generator)
baseline_policy: Pinned to (commit_sha, llm_tool, llm_model, llm_version); apples-to-apples enforced structurally
llm_role: Hot-path classifier (flow synthesis) + prose-analysis passes (intent-fit, proof-verification), all constrained by host-validated tools; structural fallback when no LLM is configured
llm_harness: `adr-mcp` — our own stdio JSON-RPC 2.0 MCP server (Rust). `adr-server` spawns it as a child process, acts as the MCP client and LLM chat client, shuttles tool calls, and writes accepted flows back to the artifact. PI was dropped (per-run extension API undocumented in pi-mono); standard MCP-over-stdio replaces it and works with any MCP-capable harness.
llm_primary_cloud: GLM-4.7 on Zhipu's coding-paas endpoint — product default for flow synthesis, intent-fit, and proof-verification. ~24s on glide-mq #181 (4 flows). GLM-4.5-air = burst, GLM-5.1 = off-peak flagship.
llm_primary_local: Qwen 3.5 27B (Q4_K_M, ~17 GB) via Ollama — offline / no-key fallback for flow synthesis. ~3m10s on glide-mq #181 (5 flows). Gemma 4 26B dropped: stalls before `finalize` on real PRs. Gemma 4 E4B dropped earlier (below structural floor).
llm_role_split: flow synthesis runs on either backend (env-pinned). Intent-fit and proof-verification default to GLM-4.7 when `ADR_GLM_API_KEY` is set — prose/semantic analysis needs strong models; small local models hallucinate here.
decision_type: Scope and tech lock for spike
---

# RFC: Architecture delta review · v0 spike · v0.3

*Status: decided · in-build (scope 5 continuation) · 2026-04-22*

---

## Pivot · v0.2

v0.1 treated the **PR** as the primary unit of review. Testing against real large TypeScript PRs (glide-mq #181 — 2.8 K additions, 1.6 K deletions, 39 files) showed this is wrong: real PRs — especially agent-authored and refactor PRs — contain **multiple independent logical flows** entangled in one diff. A single `PR → seven views` sequence flattens them into one list the reviewer has to mentally re-sort.

v0.2 shifts the product so **a flow is the primary unit of review.** A PR contains 1..N flows. Each flow is shown like v0.1 showed each PR. The "PR view" becomes the flows overview (landing + cross-flow map). Every other view (source · morph · delta · evidence · cost) operates in one of two modes: scoped to the currently selected flow, or aggregated across all flows.

The shift forces two other honest acknowledgements:

1. **Flow detection requires an LLM.** Deterministic call-graph + type-propagation clustering gets us part of the way — it's the floor, and it runs when no LLM is configured — but the architectural-cognition claim we make only holds if a model validates and re-arranges the clusters. We promote LLM from "calibration-only" to "opt-in hot-path classifier".
2. **The LLM is a classifier, not a generator.** It never writes code, never produces free-form "what this flow means" prose that isn't derived from sources we hand it. It reads our analyzed artifact over an **MCP** surface we control, returns bucket assignments + rationales referencing real nodes, and the host validates every claim against the graph before accepting it.

Backend is still Rust. Frontend is still Vite + React + Tailwind + shadcn/ui. Analyzer pipeline (tree-sitter · scip · swc/oxc/biome) is unchanged. The seven-view vocabulary stays — but every view now takes a flow scope.

Everything in this RFC overrides its v0.1 equivalent.

---

## Summary

v0 is a **standalone web review plane for agent-authored TypeScript pull requests**, targeted at teams moving from demo to production. It presents each PR as a set of detected flows (1..N) and renders a flow-scoped review surface per flow: overview, runtime flow diff, intent-vs-result morph, signed deltas, claim evidence, per-flow token-translated cost, and raw diff. Cross-flow views (all-flows map, class/module surface) are available as alternate modes. Flow detection uses hybrid deterministic clustering validated and re-arranged by a local LLM through an MCP surface we own.

Backend is Rust on top of the existing TypeScript analyzer ecosystem (swc · oxc · biome · tree-sitter · scip-typescript). The LLM side is **split by task**: flow synthesis is dual-backend (GLM-4.7 cloud by default, Qwen 3.5 27B local via Ollama as the offline fallback), while intent-fit and proof-verification default to GLM-4.7 (prose/semantic analysis). All three passes drive the same `adr-mcp` tool surface over stdio JSON-RPC. Gemma 4 (26B and E4B) was dropped after smoke tests.

Spike is ten to twelve weeks; the exit criterion is ruthless.

---

## Decisions

### 1 · Audience

**Target users for v0 are AI adopters moving from demo to production**, not enterprise teams with mature review culture.

- Teams shipping Next.js / React from agent-generated output (Cursor, Lovable, bolt, v0.dev, etc.).
- Small teams where most new lines of code originate from an LLM.
- Builders who have *not* built a review muscle and are now forced to review agent-authored PRs they cannot mentally reconstruct.
- Product moves fast, the bottleneck is trust before shipping, and the team will pay for a better way to trust code.

Not v0: enterprise teams with established code review, SOC-2-era compliance review flows, multi-reviewer sign-off policies, or code-host-native review loyalty.

Consequence: the first OSS adoption bucket has to arrive on the language these users ship in — TypeScript — and the product has to work on the *kind* of PRs they produce: large, multi-flow, refactor-shaped, sometimes incoherent.

### 2 · Tech stack

| Layer | Choice | Why |
|:---|:---|:---|
| Frontend | **TypeScript** (Vite + React + Tailwind + shadcn/ui) | Locked. |
| Backend runtime | **Rust** (tokio async, single-binary deploy) | The best TypeScript analyzer ecosystem already lives in Rust: `swc`, `oxc`, `biome`. First-class `tree-sitter` + `tree-sitter-typescript` bindings. Single-binary deploy. |
| First analyzed language | **TypeScript** | Largest overlap with the v0 persona. |
| Second analyzed language | **Deferred to v1.** Candidates by audience: TS → Rust → Go → Swift. Python deliberately deferred. |
| Analyzer pipeline | `tree-sitter-typescript` for parsing · `scip-typescript` for cross-file index (when repo has one) · within-file cross-ref by qualified name + `this.*` resolution · method-level granularity | Class methods are first-class (`ClassName.methodName`). Real TS code is class-heavy; v0.1 caught zero hunks on PR #181 because it only parsed top-level functions. Fixed. |
| LLM role | **Classifier + prose analyst via MCP.** Three passes — flow synthesis, intent-fit, proof-verification — all drive the same `adr-mcp` tool surface. LLM reads artifact through read-only tools; all mutations go through validated tool calls (`propose_flow` / `mutate_flow` / `remove_flow` / `finalize` etc.). Host validates every claim against the graph before acceptance. | Architectural cognition + semantic intent matching both need LLMs; neither gets free-form write access to source, output, or artifact. |
| LLM hosting — cloud (default) | **GLM on Zhipu's `coding-paas` endpoint** — OpenAI-compatible, Bearer auth via `ADR_GLM_API_KEY`. | Coding-plan subscription maps to `/api/coding/paas/v4/`. `glm_client.rs` normalises GLM's stringified tool-call arguments to provider-agnostic `Value` and does defensive JSON repair. |
| LLM hosting — local (fallback) | **Ollama native**, OpenAI-compatible `/v1` endpoint. | Offline / no-key tester path. Docker / WSL / vLLM reserved for scope 5+. |
| LLM model — flow-synthesis primary | **GLM-4.7 (cloud)** — ~24s on glide-mq #181, 4 flows. Parallel-batches `propose_flow`. Daily-driver tier (preserves coding-subscription quota for long-horizon use). | Default whenever `ADR_GLM_API_KEY` is set. `glm-4.5-air` = burst speed-up; `glm-5.1` = off-peak flagship (known issue: reasoning layer occasionally swallows tool emission on large schemas — not the default). |
| LLM model — flow-synthesis local fallback | **Qwen 3.5 27B dense (Q4_K_M, 17 GB, 16 K ctx)** via Ollama — ~3m10s on glide-mq #181, 5 flows. Best structural split on test set (catches `TestQueue.setBudget` as its own small flow). | Engaged when `ADR_LLM=ollama:qwen3.5:27b-q4_K_M`. Fits on an RTX 5090 laptop (24 GB VRAM) with ~6 GB headroom. |
| LLM model — intent-fit + proof-verification | **GLM-4.7** by default (`ADR_PROOF_LLM`, falls back to GLM-4.7 when only `ADR_GLM_API_KEY` is present). | Proof/intent passes read PR prose, reviewer notes, semantic claims — small local models hallucinate here (see `feedback_proof_uses_glm.md`). `from_env_proof()` warns loudly if forced onto a non-GLM backend. |
| LLM model — NOT a target | **Gemma 4 26B MoE** (stalls before `finalize` at ~4 tool calls on real PRs) and **Gemma 4 E4B** (below structural floor on smoke tests). | Dropped as product tiers. Vibe-coders don't ship on E4B-class models, and 26B MoE failing `finalize` is a hard product-level problem. Small-model tier only survives for internal tooling. |
| Strong-CLI ceiling check | claude-p · codex exec · gemini · opencode | Used for calibration probes, not the runtime build target. |
| Eval / harness | Rust CLI replaying historical PRs | Deterministic replay is essential for the spike's exit criterion. |

### 3 · Trust model

Four trust classes. Every artifact declares its class, provenance, and version. **Flow assignments are their own trust class.**

| Class | Examples | Source |
|:---|:---|:---|
| **Declared** (untrusted) | Proposal sheet · author/agent claims | Agent, human author |
| **Derived** (observed) | Architecture delta · semantic hunks · call/state/data graphs · drift markers | Parser + index + analyzers (versioned) |
| **Computed** | Cost drivers · signed deltas · confidence bands · **deterministic flow clustering** | Per-flow cost v2.3 · hybrid call-graph + type-propagation clustering |
| **Assisted** (new in v0.2) | **LLM-validated flow assignments · LLM rationales · LLM proposed entity additions** | Local LLM via MCP; every claim validated against graph before acceptance |
| **Judgment** | Verdict · waivers · merge decision | Assigned human reviewer |

Assisted content always declares `{ source: "llm:<model>@<version>", validated: true, fallback: "structural"? }`. If LLM is unavailable or its output is rejected, the artifact falls back to `computed` flow clustering labeled as such, with a banner on every view reading "structural clustering only — flows may be mismerged or split".

### 4 · Product surface — flow-first

**A flow is the primary unit of review.** A PR contains 1..N flows. A hunk can appear in multiple flows (explicitly allowed). Every hunk appears in at least one flow (host-enforced invariant).

Navigation:

- **Top spine** — the seven view names (`pr · flow · morph · delta · evidence · cost · source`) are preserved. But the **scope selector** sits alongside them: `[all flows] budget · streaming · suspend · readStream-fmt · …`.
- **Default scope** = `[all flows]` → every view renders the cross-flow overview.
- **Selected scope** = one flow → every view scopes to that flow's entities, hunks, and propagation edges.
- Alternate modes exposed on each view where useful: a **class/module surface** mode that ignores flows, a **textual diff** mode that shows raw bytes unfiltered.

The seven views per scope:

| # | View | Scoped question | Aggregated question |
|:---|:---|:---|:---|
| 01 | **pr** | Flow overview: name · rationale · entities · hunk count · cost · evidence strength | All flows: list of flows with confidence + cost bar; map mode available |
| 02 | **flow** | Runtime trajectory of the selected flow, base vs head | All flows stacked (or N overlays, reviewer switches) |
| 03 | **morph** | Intent-vs-result within this flow · replacements · claim matches | Cross-flow morph — shared entities highlighted |
| 04 | **delta** | Signed observations for this flow only | Everything, grouped by flow |
| 05 | **evidence** | Claims + evidence strength within this flow | All claims + aggregate debt |
| 06 | **cost** | Signed net for this flow alone · drivers-first | Net PR cost · per-flow breakdown bar |
| 07 | **source** | Raw diff *plus unchanged call-chain context* for this flow's entities — even unchanged lines that reach this flow's hunks are surfaced | Full textual diff, unscoped |

Cross-cutting:

- **Slide transitions between views.** Direction-aware. Animation replays on every switch.
- **Flow scope stays on view switches.** Changing from flow-2 to delta-view keeps scope = flow-2.
- **Contextual right panel only on node click.** Shows code, per-node signed cost contribution (four axes), claims touching that node, *and which flows the node participates in*.
- **No permanent sidebars.** Scope switching via inline ribbon in the spine + `/` palette.

### 4a · Flow model (new in v0.2)

A **flow** is a coherent runtime trajectory (or delta to one) that a reviewer can reason about as a unit.

Schema additions:

```jsonc
// artifact.flows: Flow[]
{
  "id": "flow-<hash>",
  "name": "multi-metric budget support",          // human-readable
  "rationale": "these methods share a …",          // one-line rationale
  "source": "llm:gemma4-26b-a4b@0.x" | "structural",
  "confidence": 0.82,                              // host-scored post-validation
  "entities": [NodeId],                            // all node ids participating in this flow
  "hunk_ids": ["hunk-…", "hunk-…"],                // every hunk must appear in ≥1 flow
  "propagation_edges": [EdgeId],                   // unchanged call-sites/refs that reach this flow's entities (N-hop)
  "extra_entities": [NodeId]?                      // entities LLM added beyond the initial computed set; host-validated
}
```

**Invariants enforced by the host (not the LLM):**

1. Every hunk appears in ≥ 1 flow (orphaned hunks trigger fallback).
2. Hunks may appear in multiple flows (explicit — no de-duplication across flows).
3. All `entities` / `extra_entities` / `propagation_edges` reference IDs that exist in the graph.
4. `name` is not a generic bucket label (`"misc"`, `"various"`, `"other"` are permitted only for the fallback bucket).
5. `confidence` is derived from {LLM self-rating, graph-structure agreement with structural fallback, proportion of LLM-added entities that validated}. Not the LLM's free report.

**Detection pipeline:**

1. **Computed (always runs):** hybrid call-graph connected components + type-propagation groupings. Result labeled `computed`.
2. **LLM assist (opt-in):** if an LLM is configured, pass the computed clusters + tool-accessible artifact to the LLM via MCP. LLM may merge, split, rename, or add entities. Host validates every mutation.
3. **Fallback:** if no LLM configured OR LLM output fails validation, artifact ships with `computed` flows, banner visible on every view.

### 5 · LLM integration — `adr-mcp` over stdio, three passes (revised)

**PI was dropped.** PI's per-run extension API was undocumented in pi-mono and we could not build a stable extension against it. We replaced it with the **standard MCP path**: a Rust crate `adr-mcp` that speaks MCP over stdio JSON-RPC 2.0. `adr-server` spawns `adr-mcp` as a child process per analysis, acts as both the MCP client and the LLM chat client, shuttles tool-call requests between the two, and writes accepted results back to `artifact.flows` / the evidence + proof channels.

Upside of the swap: the same tool surface works against any MCP-capable harness (Claude Code, Cursor, OpenCode) without code changes. Tool calls go through a provider-agnostic layer, and both cloud (GLM) and local (Ollama) flows share one validation host.

**Provider wiring (env-pinned):**

```
ADR_LLM=glm:glm-4.7                        # default when ADR_GLM_API_KEY is set
ADR_LLM=ollama:qwen3.5:27b-q4_K_M          # offline / no-key fallback
ADR_PROBE_LLM=<override>                   # probe pass; falls back to ADR_LLM
ADR_PROOF_LLM=<override>                   # intent/proof pass; defaults to glm:glm-4.7
ADR_GLM_API_KEY=<secret, never logged or cached>
ADR_GLM_URL=<override>                     # default: coding-paas endpoint
ADR_OLLAMA_URL=http://localhost:11434
ADR_OLLAMA_CTX=16384                       # 32K was needless on glide-mq
ADR_OLLAMA_PREDICT=1024
ADR_OLLAMA_TEMP=0.4                        # higher than classic — Gemma-era escape-hatch, kept for Qwen
ADR_OLLAMA_KEEP_ALIVE=10m
ADR_PROMPT_VERSION=v0.3.1                  # pre-inject + small-flow rule
```

`LlmConfig::from_env` returns `None` when no LLM is configured — pipeline falls back to structural flows with a banner. `from_env_proof` defaults to GLM-4.7 whenever the API key is present, and warns loudly if forced onto a non-GLM backend.

**GLM endpoint + auth:**
- URL: `https://api.z.ai/api/coding/paas/v4/chat/completions` (coding-plan path prefix — **not** `/api/paas/v4/`).
- Auth: `Authorization: Bearer $ADR_GLM_API_KEY`. The legacy JWT/timestamp split-key scheme is no longer required.
- Response shape: OpenAI-style; `choices[0].message.tool_calls[].function.arguments` is a **stringified JSON** (unlike Ollama's object shape). `glm_client.rs` normalises this to `Value` and does defensive JSON repair on malformed args. Hybrid-reasoning models include `reasoning_content` — ignored.
- Rate-limit handling: semaphore (`ADR_GLM_CONCURRENCY`, default 3) + bounded retry + Closed/Open/HalfOpen circuit breaker shared across synthesis / probe / proof pipelines.

**Three passes, one tool surface:**

| Pass | Primary backend | Fallback | Writes to |
|:---|:---|:---|:---|
| Flow synthesis | GLM-4.7 | Qwen 3.5 27B local | `artifact.flows[]` |
| Intent-fit (per flow) | GLM-4.7 | — (skipped if no GLM) | `artifact.claims[].intent_fit` |
| Proof-verification (per flow) | GLM-4.7 | — (skipped if no GLM) | `flow.proof` (peer of `flow.cost`, not an axis — see §9) |

**`adr-mcp` tool surface:**

*Read tools (all passes):*
- `list_hunks()` · `get_entity(id)` · `neighbors(id, hops)` · `list_flows_initial()` — structural clustering as starting point.
- For intent/proof: `get_pr_intent()` (structured `intent.json` — `{ title, summary, claims[] }`), `get_notes()` (reviewer-pasted bench output / logs / external evidence), file-read / grep / glob (lifted from the Codex CLI's Rust implementation per `feedback_reuse_codex_tools.md`).

*Mutation tools (host-validated, flow synthesis):*
- `propose_flow(name, rationale, hunk_ids, extra_entities?)` · `mutate_flow(id, patch)` · `remove_flow(id)` · `finalize()`.

*Mutation tools (intent-fit + proof):*
- `emit_intent_fit(flow_id, fit_level, rationale, claim_refs[])` · `emit_proof(flow_id, proof_level, stated_evidence?, code_evidence?, rationale)`.

**Per-tool error format (frozen at scope 3 week 6):** mutation errors return `isError: true` with a text block prefixed `ERROR: <CODE>\n<json>` so text-reading models can't miss them. Codes: `NAME_RESERVED`, `NAME_TOO_SHORT/LONG`, `RATIONALE_TOO_SHORT/LONG`, `HUNK_NOT_FOUND`, `ENTITY_NOT_FOUND`, `FLOW_NOT_FOUND`, `COVERAGE_BROKEN`, `CALL_BUDGET_EXCEEDED`.

**Host rules (not negotiated with the LLM):**
- Every hunk must be in ≥ 1 flow at `finalize()`; missing → whole-run rejection, fall back to structural.
- All referenced entity IDs must exist in the graph; no invented entities.
- Reserved generic names (`misc`, `various`, `other`) rejected except for the structural-fallback bucket.
- Call budget ≤ 200 tool invocations per run.
- Whole-run rejection on any invariant violation; structural flows ship with a visible banner.

**Per-run lifecycle:**
- `adr-server` parses the graph, writes an artifact, spawns `adr-mcp` (stdio), and opens an LLM chat session against the configured provider.
- Synthesis pass first → flows stable → intent-fit pass (per flow) → proof-verification pass (per flow). Each pass is a clean chat session with its own context; they communicate only through the mutating tool calls.
- Events tee'd to the frontend via SSE so the reviewer sees "detecting flows… · intent-fit · proof" progress. UI copy describes WORK, not the model (per `feedback_no_model_names_in_ui.md`).
- Baselines pin `(commit_sha, llm_tool, llm_model, llm_version, flow_synthesis_model, proof_model)`; any drift refuses the delta.

**GLM tool-call XML drift (known issue):** GLM-4.6/4.7 occasionally leak native `<tool_call>…</tool_call>` XML into content instead of using OpenAI `tool_calls[]`. The client-side parser in `tool_call_drift.rs` rehydrates these into proper tool calls (research consensus over prompt-nudging). Don't describe the tool-call format in the prompt — it increases drift.

### 6 · Visual / design language

Locked in v0.1, carried forward:

| | |
|:---|:---|
| Surface | Near-white (`#FCFBF8`) in light / deep neutral in dark · hairline dividers · no grid/noise |
| Typography | Inter (body and UI) · JetBrains Mono (every identifier, path, number, keyboard glyph) · 3 weights only (400/500/600) · no italics |
| Emphasis | Color and weight |
| Copy voice | Terse, technical, data-forward · labels not headlines |
| Color | Green (add) · red (rose, remove) · amber (architectural overlay, drift, partial) · neutral grays · one saturated accent to-be-chosen on a data-heavy view |
| Motion | 220–340 ms ease-out slides · animation replays on switch · chip fade on strip hover |
| Viz share | 80–90 % of screen when active · sidebars on demand only |

### 7 · Semantic hunk types — v0

Ship (unchanged from v0.1 except for method-level granularity):

- **call / control flow** — function + method call graph delta
- **state transition** — named state machine detection
- **api surface** — HTTP status codes, request/response schemas, type-level exports, exported function + method signatures
- **lock / resource flow** — idempotency primitives, cache/lock introductions
- **data flow** — event/schema changes, basic taint endpoints
- **docs / runbook alignment** — code↔doc link deltas
- **deletion / cleanup** — dead-code removal, stub removal

Defer to v1: full taint analysis, cross-service data flow, whole-program graph diffing.

### 8 · Evidence classes — v0 (per flow)

Evidence attaches to claims within a flow, not to the PR globally.

| Class | v0 minimum | Tooling |
|:---|:---|:---|
| **PERF** | Bench harness output with baseline | Harness in-repo · CI replay |
| **CONC** | Optional in v0 · required in v1 for state-machine hunks | TLA+ / Apalache stub |
| **DATA** | OpenTelemetry trace set over replay | otel collector |
| **API** | Contract-test pass on the consumer map | Pact-style or in-repo |
| **LOCK** | Unit test + asserted comment | vitest / jest |

Proof debt is a first-class, **per-flow** field — carried on `flow.proof`, not inside `flow.cost.axes`. See §9.

### 9 · Cost model — v0 (per-flow primary, aggregate secondary)

**Core principle stands.** We are measuring LLM cognition: how expensive is it for the next session to safely continue work on the affected flow? Now flow is literal.

- **Per-flow cost** is primary. Each flow has its own three signed navigation axes (`continuation · runtime · operational`), driver breakdown, and net. **Proof lives outside cost** on the flow (`flow.proof`, peer of `flow.cost`) — cost is navigation movement, proof is evidence of stated intent, and mixing them confused reviewers about what each bar meant (correction from v0.2, which listed proof as a fourth axis).
- **Aggregate PR cost** is the sum, shown on the all-flows overview.
- **Drivers and baselines are unchanged from v0.1.** `grep_friendliness`, `file_read_cost`, `scopes_added`, `logical_steps`, `retrieval_ambiguity`, `docs_alignment` — each a continuous score per flow.
- **Baseline pinning** remains `(commit_sha, llm_tool, llm_model, llm_version)` and includes the **flow-synthesis model** now. A baseline mismatch includes flow-synthesis model drift.
- **LLM CLIs integration** is preserved; we now have both `adr-mcp`-driven synthesis *and* calibration-time probes. Both use the same local-CLI or Ollama runners.
- **Repo-relative only.** No cross-repo universal number.

### 10 · What ships in v0

- Flow-first seven-view review surface.
- TypeScript analyzer pipeline with method-level granularity + `this.*()` resolution + full-signature capture for multi-line method declarations.
- Hybrid deterministic flow clustering (call-graph components + type-propagation) as the floor.
- `@adr/pi-extension` — PI extension exposing the tools listed in §5.
- Rust-side PI runner: spawns PI with Ollama config, pipes artifact, tees tool-call progress to frontend SSE; Gemma 4 26B MoE (primary) and stronger models supported out-of-box.
- Evidence collection for PERF, DATA, API, LOCK — per-flow.
- Cost model v2.3 per-flow with PR-aggregate rollup.
- `adr baseline` with pinned `(sha, llm_tool, llm_model, llm_version)` including flow-synthesis model.
- `adr-llm` adapters to local CLIs (ceiling-check targets: claude · codex · gemini · opencode; primary: Ollama direct).
- Proposal sheet support (unchanged from v0.1).
- Per-node panel: code, cost contribution, claims, **flow memberships**.
- Raw-diff view with proper gutter, markers, hunk headers, architectural overlay strip.
- `/` palette for flow switching and navigation.
- Self-hostable distribution: tester runs orchestrator + frontend + Ollama locally; no pricing, no accounts, no hosted layer in v0.

### 11 · Explicitly deferred to v1+

- **Focus mode** (push unrelated nodes aside).
- **Agent-packet projection** (machine-readable graph serialization).
- **Per-PR runtime LLM cognition probe** (opt-in high-confidence mode; ~$0.50/PR cloud cost).
- **TLA+ / Apalache as a hard gate** for state-machine hunks.
- **Full data-flow / taint analysis**.
- **Python / Go / Java / Rust analyzer pipelines**.
- **Narrative export** (GIF / MP4 replay).
- **GitHub App distribution**.
- **Hosted SaaS tier**: per-repo + per-PR pricing, server-side repo indexing, retained multi-baseline caching, cheaper-model routing on our side.
- **vLLM / multi-GPU serving**: Ollama is sufficient for v0; vLLM becomes relevant only if 70B+ dense models enter the target band.
- **LLM-in-calibration-harness**: the calibration pipeline (scope 4) uses local CLIs as before; MCP-driven flow synthesis is runtime, not calibration.

---

## Spike plan

*Full re-shape lives in `spike-plan.md`. Outline:*

| Week | Deliverable |
|:---|:---|
| 1–2 | ✅ Versioned graph + hunk schema. Three hunk types (call, state, api) with class-method support. |
| 3–4 | Orchestrator, frontend scaffold, Source view with syntax highlighting and architectural overlay. Flow ribbon in spine (visible but inert). |
| 5–6 | **`adr-mcp` crate + deterministic flow clustering + Ollama adapter + Gemma 4 26B MoE smoke test.** |
| 7–8 | Flow-scoped renderers across the seven views. Per-flow cost model. Evidence collectors. |
| 9–10 | Evaluation: glide-mq (primary, 5 PRs) + Inngest (secondary, 5 PRs) + 3 seeded architectural bugs. Reviewer A/B vs raw-diff. |
| 11–12 | Narrow or kill. |

### The eval question (updated for v0.2)

> *For which TypeScript PR classes does the v0 flow-first surface let a reviewer reach correct per-flow verdicts faster and more confidently than raw-diff review — and where does the LLM-assisted flow detection vs structural fallback make a user-visible difference?*

Three nested sub-questions:

1. **Does it work at all?** Reviewers complete per-flow verdicts without hand-holding.
2. **Does it beat raw diff?** Same reviewers, same PRs, A/B against raw diff.
3. **Does the LLM assist matter?** Side-by-side comparison of LLM-validated flows vs structural-only flows on the same PRs.

### Eval set (updated for v0.2)

- **Primary calibration repo: [glide-mq](https://github.com/avifenesh/glide-mq).** Real vibe-coded TS codebase; real multi-flow refactor PRs (#181 thinking-model support, #192 proxy parity, #205 flow HTTP API, #207 ordering deadlock fix, #193 suspend timeouts). 5 PRs.
- **Secondary calibration repo: [Inngest](https://github.com/inngest/inngest-js).** 5 historical PRs with state-machine / retry / durable-workflow flavor.
- **3 seeded bug PRs** on a fork — concurrency bug · state drift · proof debt (unchanged from v0.1).

### Exit gate (updated for v0.2)

Proceed to full v1 RFC *only if all hold*:

1. Reviewers prefer the v0 flow-first surface to raw-diff review on ≥ **60%** of PR classes. **And**
2. Reviewers catch all three seeded bugs **faster** on the v0 surface. **And**
3. **LLM-assisted flow detection ≥ structural-only** on at least one metric (time-to-verdict, confidence, or correctness) with statistical significance on the eval set, tested on **both** the cloud backend (GLM-4.7) and the local fallback (Qwen 3.5 27B). The cloud path is the product default; the local path is the offline promise.

Not all three: stop or narrow.

### Kill conditions

- Observed-graph pipeline requires build-env integration for > 30 % of eval PRs.
- Cost-model drivers visibly wrong on > 25 % of PRs.
- Reviewers report the flow-first surface is slower than raw-diff review *even with* LLM assist.
- GLM-4.7 fails to produce valid flow assignments (host rejects or output falls below structural quality) on > 25 % of eval PRs — this is the cloud product default, failing it is a product-level stop. AND
- Qwen 3.5 27B local fails the same bar on > 40 % of eval PRs — we allow a wider floor on the offline path, but past that threshold the "works offline" promise is hollow.

---

## Resolutions carried from v0.1

- Claim taxonomy ownership → base taxonomy + per-repo extensions.
- Agent-authored detection → dismissed.
- Eval confidentiality → private fork during eval window.
- Proposal-sheet adoption → CLI scaffold + bot fallback; never hard-require.

## New resolutions in v0.2

- **Flow detection ownership**: hybrid deterministic (call-graph + type-propagation) is the floor, always computed. LLM is opt-in via Ollama or configured local CLI. No v0 code path assumes LLM is present.
- **LLM scope**: classifier only, via MCP-exposed tools. No free-form generation. Host validates every mutation.
- **Product model split**: GLM-4.7 cloud is the default flow-synthesis backend (8× faster than local with comparable quality on glide-mq #181). Qwen 3.5 27B local via Ollama is the offline fallback — the "no key needed" promise. Intent-fit and proof-verification are GLM-only by default; prose-analysis tasks need strong models. Gemma 4 (26B and E4B) dropped.
- **Harness**: PI extension path abandoned; replaced with `adr-mcp` stdio JSON-RPC. Same tool contract works for any MCP-capable client.
- **Hosted SaaS still deferred**: v0 self-hostable; hosted tier is v1+ with per-repo indexing, retained baselines, and model routing.

---

## Non-goals

- Replace Git, PR systems, CI, or existing code hosts.
- Eliminate raw-diff review entirely.
- Let the generator agent define the "right" architectural direction.
- Pretend there is a repo-agnostic universal complexity score.
- Block progress behind verification tooling.
- Support every language or every delta type in v0.
- **Distinguish agent-authored from human-authored PRs.** (Preserved from v0.1.)
- **Have the LLM rewrite the code, the artifact, or the hunks.** It classifies and validates; nothing else.
- **Force users into a PR shape that matches one flow.** The product explicitly targets multi-flow PRs.

---

## Appendix A · technology seeds

Unchanged core (tree-sitter, scip, swc, oxc, biome, tokio, scip-typescript, SARIF, OpenTelemetry, OPA, SLSA, Apalache, Glean, Joern, CodeQL, Semgrep, tree-sitter-graph) carried from v0.1.

v0.2 additions:

| ID | Why | Source |
|:---|:---|:---|
| R19 | Ollama — local model runtime, OpenAI-compat (offline flow-synthesis fallback) | https://ollama.com/ |
| R20 | Qwen 3.5 27B dense — local fallback for flow synthesis | https://huggingface.co/Qwen |
| R21 | GLM-4.7 / GLM family via Zhipu coding-paas — cloud primary for all three passes | https://z.ai/ |
| R22 | Model Context Protocol — our `adr-mcp` crate implements MCP over stdio JSON-RPC 2.0 | https://modelcontextprotocol.io/ |
| R23 | OpenAI function-calling spec — tool-call contract shared by GLM and Ollama | https://platform.openai.com/docs/guides/function-calling |
| R24 | Codex CLI (Rust read/grep/glob implementations lifted for agent file tools, per `feedback_reuse_codex_tools.md`) | https://github.com/openai/codex |

## Appendix B · changes from pre-RFC

*(carried from v0.1)*

## Appendix D · v0.3 delta from v0.2

- **LLM harness**: PI + `@adr/pi-extension` dropped (per-run extension API undocumented in pi-mono). Replaced with `adr-mcp` — Rust crate speaking MCP over stdio JSON-RPC 2.0. `adr-server` spawns it as a child per analysis and acts as MCP client + LLM chat client.
- **Backend split by task**:
  - Flow synthesis: **GLM-4.7 cloud** primary (~24s / 4 flows on glide-mq #181) · **Qwen 3.5 27B local** fallback (~3m10s / 5 flows).
  - Intent-fit + proof-verification: **GLM-4.7** default (`from_env_proof` warns on non-GLM backend) — prose/semantic analysis needs strong models.
- **Gemma 4 fully dropped** as a product target. 26B MoE stalls before `finalize` on real PRs; E4B was already below the structural floor.
- **GLM wiring**: coding-paas endpoint (`/api/coding/paas/v4/`), Bearer auth, OpenAI-style response shape with stringified tool-call arguments (normalised by `glm_client.rs`). Legacy JWT split-key scheme retired.
- **GLM rate-limit handling**: semaphore (`ADR_GLM_CONCURRENCY`, default 3) + bounded retry + Closed/Open/HalfOpen circuit breaker shared across synthesis / probe / proof.
- **Tool-call XML drift**: client-side rehydrator for GLM's leaked `<tool_call>` XML (`tool_call_drift.rs`) rather than prompt nudging.
- **Env knobs renamed / added**: `ADR_LLM`, `ADR_PROBE_LLM`, `ADR_PROOF_LLM`, `ADR_GLM_API_KEY`, `ADR_GLM_URL`, `ADR_GLM_CONCURRENCY`. Bare `ADR_GLM_API_KEY` auto-defaults `ADR_LLM` to `glm:glm-4.7`.
- **Baseline pin extended** to include `proof_model` alongside `flow_synthesis_model`.
- **Exit gate + kill conditions** rewritten around the dual backend: LLM-assist-vs-structural must hold on both GLM and Qwen paths; GLM > 25 % / Qwen > 40 % reject rates are kill triggers.
- **Scope-5 status**: probe baselines + per-flow signed cost delta live (`adr-probe` + `adr-cost`). Intent/proof LLM passes are scope-5 continuation — planned, not built.

---

## Appendix C · v0.2 delta from v0.1

- **Primary unit of review: PR → flow.** Every view now takes a scope.
- **LLM role: calibration-only → opt-in hot-path classifier via MCP.** Validated, not trusted; structural fallback retained.
- **Schema addition: `artifact.flows[]`.** Host enforces every-hunk-in-at-least-one-flow.
- **Hunk rule: hunks can appear in multiple flows.** Explicit, not a bug.
- **Parser gained class-method granularity** (`ClassName.methodName`), `this.*()` resolution, full multi-line signature capture. Tested on glide-mq PR #181: v0.1 produced 0 hunks; v0.2 produces 12.
- **LLM backend split**: GLM-4.7 cloud is the primary for all three LLM passes (synthesis + intent-fit + proof); Qwen 3.5 27B via Ollama is the offline synthesis fallback. Local CLI adapters (claude, codex, gemini, opencode) remain for ceiling checks. PI dropped; `adr-mcp` stdio JSON-RPC replaces it.
- **Cost model per-flow primary, aggregate secondary.**
- **Calibration repo list expanded**: glide-mq as primary (real vibe-coded TS with real multi-flow refactors), Inngest as secondary.
- **Exit gate adds LLM-assist-matters check** against the structural floor.
- **Kill conditions updated**: GLM-4.7 cloud > 25 % reject rate is a product stop; Qwen 3.5 27B local > 40 % reject rate hollows the offline promise. Gemma-4 failure condition retired — the model itself is no longer a target.
- **Non-goal added**: LLM never writes code or free-form content into the artifact.
- **Audience framing strengthened**: vibe-coding / demo-to-prod reviewers are the explicit target, including the large, multi-flow PR shape they produce.
