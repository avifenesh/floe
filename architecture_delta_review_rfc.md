---
title: "RFC: Architecture delta review · v0 spike"
date: 2026-04-17
status: Decided · ready for build
supersedes: architecture_delta_review_pre_rfc.md
first_analyzed_language: TypeScript
backend_runtime: Rust (tokio)
frontend: Vite + React + Tailwind + shadcn/ui
primary_substrate: Self-hostable web review app (testers only in v0)
calibration_repo: Inngest (inngest/inngest-js)
proposal_sheet_format: Explicit YAML block in PR description
cost_model_principle: Deterministic drivers grounded in LLM-navigation research; LLM CLIs invoked only at calibration / opt-in validation, never on the hot path
baseline_policy: Pinned to (commit_sha, llm_tool, llm_model, llm_version); apples-to-apples enforced structurally
llm_integration: Local CLIs (claude -p · codex exec · gemini · opencode run); users bring their own
decision_type: Scope and tech lock for spike
---

# RFC: Architecture delta review · v0 spike

*Status: decided · ready for build · 2026-04-17*

---

## Summary

v0 is a **standalone web review plane for agent-authored TypeScript pull requests**, targeted at teams moving from demo to production — not at organizations that already review well. The product is a sequenced, seven-view review surface that presents (1) the PR summary, (2) a side-by-side runtime flow diff, (3) an intent-vs-result morph layer, (4) signed deltas, (5) claim-level evidence, (6) token-translated cost drivers, and (7) raw diff as drill-down. Backend is Rust on top of the existing TypeScript analyzer ecosystem (swc · oxc · biome · tree-sitter · scip-typescript). The spike is ten to twelve weeks; the exit criterion is ruthless.

This RFC locks the scope, tech stack, audience, and visual model of the spike. Everything out of scope is either named as a v1+ deferral or listed as an open question.

---

## Decisions

### 1 · Audience

**Target users for v0 are AI adopters moving from demo to production**, not enterprise teams with mature review culture.

- Teams shipping Next.js / React from agent-generated output (Cursor, Lovable, bolt, v0.dev, etc.).
- Small teams where most new lines of code originate from an LLM.
- Builders who have *not* built a review muscle and are now forced to review agent-authored PRs they cannot mentally reconstruct.
- Product moves fast, the bottleneck is trust before shipping, and the team will pay for a better way to trust code.

Not v0: enterprise teams with established code review, SOC-2-era compliance review flows, multi-reviewer sign-off policies, or code-host-native review loyalty. Those are v2+.

Consequence: the first OSS adoption bucket has to arrive on the language these users ship in.

### 2 · Tech stack

| Layer | Choice | Why |
|:---|:---|:---|
| Frontend | **TypeScript** (Vite + React, no framework opinion beyond that for v0) | Obvious. Mature, cheap to hire for, works well for any later substrate swap. |
| Backend runtime | **Rust** (tokio async, single-binary deploy) | The best TypeScript analyzer ecosystem already lives in Rust: `swc`, `oxc`, `biome`. First-class `tree-sitter` and `tree-sitter-typescript` bindings. Orchestration and LLM-fanout happens mostly on I/O; tokio handles it cleanly. Single-binary deploy into a GitHub App in v1 is friction-free. |
| First analyzed language | **TypeScript** | Largest overlap with the v0 persona (Next.js, AI SDK builders, Cursor/Lovable output). Weak-concurrency tax is acceptable because this persona's pain is API contract · data flow · hidden state · runtime cost, not state-machine soundness. |
| Second analyzed language | **Deferred to v1.** Decided after v0 adoption signal. | Candidates ranked by audience: TS → Rust → Go → Swift. **Python deliberately deferred** — dynamic typing taxes our best claim classes and the OSS bucket for Python review tooling is crowded (ruff, bandit, pyright, sonar-python). |
| LLM integration | OpenAI-compatible client called from the Rust orchestrator | LLMs do *not* produce the observed delta. They normalize proposal-sheet wording, author reviewer-prompt templates, and surface claim-to-evidence gaps. Observed architecture comes from the analyzer pipeline only. |
| Eval / harness | Rust CLI replaying historical PRs | Deterministic replay is essential for the spike's exit criterion. |

Why not Go for backend: general "Go for code tools" advice doesn't apply here because we are not analyzing Go. For TS-first, Rust's ecosystem (swc · oxc · biome) is where the crowd and the good tooling already are. We would end up shelling from Go into Rust binaries anyway.

Why not Python on the server: we'd be serializing AST graphs under the GIL while competing with LLM calls in the same process. Python belongs strictly as a potential *analyzed target* in a later version, not the runtime.

### 3 · Trust model

Four trust classes. Every artifact declares its class, provenance, and version.

| Class | Examples | Source |
|:---|:---|:---|
| **Declared** (untrusted) | Proposal sheet · author/agent claims · stated benefits | Agent, human author, issue/design doc |
| **Derived** (observed) | Architecture delta · semantic hunks · call/state/data graphs · drift markers | Parser + index + analyzers (versioned) |
| **Computed** | Cost drivers · signed deltas · confidence bands | Repo-calibrated cost model v2.3 |
| **Judgment** | Verdict · waivers · merge decision | Assigned human reviewer |

No declared field is promoted to derived without explicit normalization and provenance. No computed number is displayed above its confidence threshold.

### 3a · Proposal sheet format

**Explicit YAML block** in the PR description, fenced with `adr-proposal`. Format locked for v0:

```yaml
```adr-proposal
direction: >
  one or two sentences on the architectural move.
claims:
  - id: ARCH.DECOUPLE
    note: handler ack is decoupled from the payment retry
  - id: RETRY.EXPLICIT
    note: retries become named states, not a loop
  - id: RETRY.BOUNDED
    evidence: static-bound-analysis
  - id: LATENCY.UNBLOCK
    evidence: bench
  - id: OBS.PER_ATTEMPT
    evidence: otel-traces
non_goals:
  - durable replay across process restarts (v1)
```
```

- `direction` — free text, one or two sentences.
- `claims[].id` — drawn from a base taxonomy (ARCH · RETRY · LATENCY · CONC · DATA · API · LOCK · IDEM · OBS · DOCS). Per-org extension allowed in v1.
- `claims[].evidence` — optional. If set, names the minimum evidence class expected; analyzer fails loudly if not produced.
- `non_goals` — things the PR is *not* trying to do. Useful for suppressing false drift flags.

**Authoring flow**: a Rust CLI (`adr scaffold`) reads the PR description and emits a draft YAML block from free-text using an LLM pass. The author edits and commits. The analyzer treats the committed YAML as ground-truth *declared intent* — still class `declared`, still untrusted until verified against observed.

If no block is present, v0 degrades: the morph view shows "no proposal · drift detection disabled," and the surface still functions as a semantic diff without claim-matching.

### 4 · Product surface — seven views

Sequenced focus · one question per view · top spine navigation · visible prev/next arrows on viewport edges · keyboard `←/→` · `/` command palette.

| # | View | Question | Primary artifact |
|:---|:---|:---|:---|
| 01 | **pr** | what is this PR, at a glance? | Metadata + four headline stats (handler Δ, cost Δ, drift count, proof debt) + hunks summary |
| 02 | **flow** | how does a request traverse base vs head? | Two canvases side by side, animated packet(s) play the runtime path through each |
| 03 | **morph** | which components replaced which? which proposal claims matched? | Intent-vs-result rows + component replacement table |
| 04 | **delta** | what signed observations changed? | Seven to eight delta cards, color-coded, click-to-drill |
| 05 | **evidence** | which claims are backed, and how strongly? | Claim rows with strength pips + provenance |
| 06 | **cost** | what is the signed delta, driver by driver, in concrete tokens? | Four axes, drivers first, net secondary, `ctx-tok ≈ LLM tokens` translation |
| 07 | **source** | show me the raw diff for a given file | Proper gutter diff · line numbers · markers · hunk headers |

Cross-cutting:
- **Slide transitions between views.** Forward slide right, backward slide left. Direction is derived from index change. Animation replays on every switch (forced reflow).
- **Contextual right panel only on node click.** Shows code at node, per-node signed cost contribution (four axes), claims touching that node. Dismissable with `esc` or click-outside.
- **No permanent sidebars.** Hunk switching lives in a slash-palette, not a column.

### 5 · Visual / design language

| | |
|:---|:---|
| Surface | Near-white (`#FCFBF8`) · hairline dividers only · no decorative texture · no grid/noise |
| Typography | Inter (body and UI) · JetBrains Mono (code · tags · IDs · numerics) |
| Emphasis | Color and weight — **not** italic serif |
| Copy voice | Terse, technical, data-forward — not editorial · no HR-slide prose · labels not headlines |
| Color | Palette restrained: green (good) · red (bad) · amber (drift · partial) · navy (declared · referential) · ink (neutral) |
| Motion | 220–340ms ease-out slides · animation replays on switch (no stuck frame) |
| Viz share | 80–90% of screen when active · sidebars only on demand |

Explicitly killed in v0 after iteration: paper/drafting aesthetic · Instrument Serif · one-page cockpit · two permanent sidebars · italics for emphasis · editorial subtitles. All were defaults. None were decisions.

### 6 · Semantic hunk types — v0

Ship:

- **call / control flow** — function call graph delta, handler restructuring
- **state transition** — named state machine detection, state additions / removals, drift in enumerated states
- **api surface** — HTTP status code set, request/response schema, type-level exports
- **lock / resource flow** — idempotency primitives, cache/lock introductions (e.g. Redis SETNX, mutex scope)
- **data flow** — event/schema changes, basic taint endpoints
- **docs / runbook alignment** — code↔doc link deltas
- **deletion / cleanup** — dead-code removal, stub removal

Defer to v1:
- Full taint analysis
- Cross-service data flow
- Whole-program graph diffing

### 7 · Evidence classes — v0

| Class | v0 minimum | Tooling |
|:---|:---|:---|
| **PERF** | Bench harness output with baseline | Harness in-repo · CI replay |
| **CONC** | Optional in v0 · *required* in v1 for state-machine hunks | TLA+ / Apalache stub |
| **DATA** | OpenTelemetry trace set over replay | otel collector |
| **API** | Contract-test pass on the consumer map | Pact-style or in-repo |
| **LOCK** | Unit test + asserted comment | vitest / jest |

Proof debt is a first-class field: if the minimum for a claim class is not met, it shows as debt and contributes to the cost model — not a blocker in v0.

### 8 · Cost model — v0

**Core principle.** The thing we are measuring is LLM cognition: how expensive is it for the *next* session to safely continue work on the affected flow? We commit to measuring this honestly. Deterministic drivers are the cheap, per-PR proxy. Real LLM sessions (via locally-installed CLIs) validate the proxy. Baselines are pinned so that every comparison is apples-to-apples — or refused.

- **Four signed axes**: continuation · runtime · operational · proof. Drivers listed before totals. Net shown only when confidence ≥ 0.70.

- **Drivers are grounded in LLM-navigation research** — each names a specific way LLMs read code, not an arbitrary feature:
  - `grep_friendliness` — change in recall for symbol / error-message queries (LLMs grep; they don't go-to-definition)
  - `file_read_cost` — files the LLM must open to reconstruct the affected flow (BFS over the call graph, whole-file reads)
  - `scopes_added` — new modules / functions / types (expands the search space the next session must traverse)
  - `logical_steps` — call depth × state transitions × async boundaries (sequential reasoning hops)
  - `retrieval_ambiguity` — symbols sharing prefixes or near-names with the affected symbol
  - `docs_alignment` — coherence between runbook paragraphs and code paths
  
  Each driver returns a **continuous score**, not a binary fire-or-not. Coefficients in `coefficients.toml` are fitted per-repo against validation-probe ground truth.

- **Baselines are pinned.** Every repo's baseline is recorded as `(commit_sha, llm_tool, llm_model, llm_version)`, persisted at `.adr/baseline-<sha>.json` and committed to the repo. On every subsequent analysis, if the triple has drifted — the repo has new commits, the reviewer's CLI changed, a model version bumped — the UI **refuses to show a comparison delta** and surfaces `re-baseline required`. Apples-to-apples is structural, not optional.

- **LLM CLIs, not bespoke runtime.** We do not build an LLM integration layer. We integrate with locally-installed CLIs:
  - v0 adapters: `claude -p` · `codex exec -p` · `gemini` · `opencode run`
  - detection via `$PATH` + `--version` probe; configuration via `.adr/llm.toml`
  - users who have none of these can still use the product — cost renders with `confidence: unknown` and deterministic-only drivers
  - local-LLM (ollama, llama.cpp) adapters are v1 work, picked after we see what testers actually run

- **Validation probe is opt-in, separate from main measurement.**
  - **Default**: 3 generic follow-up prompts per detected codebase type (Next.js route handler · workflow lib · type-safe API · serverless · React app), using a classifier over `package.json`, `tsconfig.json`, and top-level structure.
  - **Opt-in**: `adr baseline --propose-questions` runs an LLM over the repo once, proposes 3 repo-specific follow-up tasks, saves to `.adr/questions.toml`. Repo-specific ≻ generic.
  - **Opt-out**: user skips LLM entirely; cost is deterministic-only with `confidence: unknown`.
  - The probe never drives per-PR cost directly. It runs during calibration to produce ground truth for coefficient fitting, and periodically (scheduled or on-demand) to refresh the repo's confidence σ.

- **Concrete token translation stays honest.** The cost-view strip reads: *"structural approximation of LLM continuation cost; calibrated against N real LLM sessions in {repo}; residual σ = M tokens."* No pretending the number is the ground truth.

- **Repo-relative only.** No cross-repo universal number. No leaderboards, ever.

### 9 · What ships in v0

- Seven-view review surface described above.
- TypeScript analyzer pipeline: `tree-sitter-typescript` + `scip-typescript` for index; `swc` or `oxc` for parse + control-flow graph; custom Rust passes for semantic hunk extraction.
- Evidence collection for PERF, DATA, API, LOCK.
- Cost model v2.3 — deterministic drivers grounded in LLM-navigation research, per-repo calibrated, drivers-first, signed.
- **`adr baseline`** — pinned baseline per `(sha, llm_tool, llm_model, llm_version)`, stored in `.adr/baseline-<sha>.json`.
- **`adr-llm`** — adapters to locally-installed CLIs: `claude -p`, `codex exec -p`, `gemini`, `opencode run`.
- Optional `adr baseline --propose-questions` for repo-specific validation prompts.
- Per-node code + per-node signed cost in contextual panel.
- Raw-diff view with proper gutter, markers, hunk headers.
- Slash-command palette + visible edge navigation.
- **Self-hostable distribution**: tester runs orchestrator + frontend locally, brings their own CLI. No pricing, no accounts, no hosted layer in v0.

### 10 · Explicitly deferred to v1+

- **Focus mode** (push unrelated nodes aside, keep edges faded-but-live).
- **Agent-packet projection** (machine-readable graph serialization for downstream agents).
- **Per-PR runtime LLM probe** (cold-start continuation at PR time, not just calibration time). Opt-in "high-confidence mode" in v1, ~$0.50/PR. Calibration-time probes are already in v0.
- **TLA+ / Apalache as a hard gate** (v0: optional; v1: required for state-machine hunks).
- **Full data-flow / taint analysis** as a first-class hunk type.
- **Python / Go / Java / Rust analyzer pipelines.** Decided after v0 signal.
- **Narrative export** (GIF / MP4 replay of the flow).
- **GitHub App distribution** (v0 is standalone web; App in v1).
- **Hosted SaaS tier**: per-repo + per-PR pricing, server-side repo indexing, retained multi-baseline caching, cheaper-model routing on our side. v0 is self-hostable only; hosting is where we optimize cost later.
- **Local-LLM adapters** (ollama, llama.cpp, etc.). Wait for v0 signal on what testers actually run before adding.

---

## Spike plan

### Timeline · 10–12 weeks

| Week | Deliverable |
|:---|:---|
| 1–2 | Versioned graph + hunk schema. `tree-sitter-typescript` + `scip-typescript` wired. First three hunk types (call, state, api) land with tests. |
| 3–4 | Control-flow + basic state-machine detection. Rust orchestrator skeleton. TS frontend scaffold. |
| 5–6 | Seven views built. Slide transitions. Node→panel flow. Raw diff with gutter. |
| 7–8 | Evidence collection pipelines (bench, trace, contract, unit). Cost model v2.3 calibrated against 24 historical PRs in chosen repo. |
| 9–10 | Evaluation: 10 historical PRs + 3 seeded bug PRs, reviewer A/B against raw-diff baseline. |
| 11–12 | Narrow or kill based on eval signal. If positive, draft v1 scope. |

### The eval question

> *For which TypeScript PR classes does the v0 architectural surface let a reviewer reach a correct verdict faster and more confidently than raw-diff review — and are there classes where it doesn't?*

The exit gate (below) is a **pass/fail**; the question is what the gate is measuring. Three nested sub-questions, in order:

1. **Does it work at all?** Reviewers complete a full verdict on real PRs without hand-holding or missing state.
2. **Does it beat raw diff?** Same reviewers, same PRs, in a side-by-side protocol: time-to-verdict, confidence, and correctness.
3. **On which classes?** Is there a clearly-winning subset (refactors · state-machine hunks · async introductions · idempotency changes) and a clearly-neutral subset (renames · small mechanical diffs · pure docs)?

### Eval set

- **Calibration repo: [Inngest](https://github.com/inngest/inngest-js)** — chosen for its concentration of the patterns our strongest hunk types exercise (retries, state machines, durable workflows, async/event-driven). Sampling 10 historical PRs spanning the last 12 months.
- **3 seeded bug PRs** constructed against a fork of the same repo:
  - Concurrency bug in an async retry / queue handler (e.g. missing idempotency guard across retries).
  - State drift (agent adds a state to an internal machine that isn't in the proposal sheet).
  - Proof debt (claim made, required evidence intentionally not attached).

### Protocol

Two reviewers per PR. One reviews on raw diff (GitHub). One reviews on the v0 surface. Swap halfway. Record for each reviewer:

- time-to-verdict (direction · implementation · evidence · cost axes, individually)
- verdict confidence (self-reported)
- verdict correctness (against ground truth, especially for the seeded bugs)
- post-hoc preference rating

### Exit gate

Proceed to full v1 RFC *only if both hold*:

1. Reviewers prefer the v0 surface to raw-diff review on ≥ **60%** of PR classes covered by the eval set. **And**
2. Reviewers catch all three seeded bugs **faster** on the v0 surface than on raw-diff baseline.

Not both: stop or narrow. Don't expand scope in week 11 to chase a near-miss.

### Kill conditions

- The observed-graph pipeline requires build-environment integration for more than 30% of the eval PRs. (Too brittle for v0.)
- Cost-model drivers are visibly wrong on more than 25% of PRs (signs flipped, drivers missing). (Model needs re-think before product.)
- Reviewers report the surface is slower than raw-diff review without matching benefit.

---

## Resolutions (formerly open questions)

All v0 open questions resolved as of 2026-04-17.

1. **Claim taxonomy ownership** → ship a **base taxonomy** (10–15 IDs covering v0 hunk classes: `ARCH` · `RETRY` · `LATENCY` · `CONC` · `DATA` · `API` · `LOCK` · `IDEM` · `OBS` · `DOCS`) with **per-repo extensions** via `.adr/taxonomy.yaml` at repo root. Per-org registry deferred to v2. Draft taxonomy due week 2.
2. **Agent-authored detection** → **dismissed, not resolved.** The v0 audience ships mostly agent-authored code by default. There is no meaningful "agent vs human" signal to surface or act on in this user base. See non-goals.
3. **Eval confidentiality** → **private fork** during the eval window (weeks 9–10). Reviewers don't know which PRs are seeded until the debrief. After the window, fork + aggregate results + protocol are open-sourced.
4. **Proposal-sheet adoption** → **CLI scaffold** (`adr scaffold`) drafts YAML from PR description; author reviews and commits. **Bot PR-comment fallback** for repos without the CLI. **Never hard-require** in v0. Adoption rate tracked as a secondary eval metric; if < 30% of authors write the block, morph is a v1 concept, not v0.

---

## Non-goals

- Replace Git, PR systems, CI, or existing code hosts.
- Eliminate raw-diff review entirely — raw diff is drill-down, not front.
- Let the generator agent define what the "right" architectural direction is.
- Pretend there is one repo-agnostic, universal complexity score meaningful everywhere.
- Require full formal proof for every change or block progress behind verification tooling.
- Support every language or every kind of architecture delta in v0.
- **Distinguish agent-authored from human-authored PRs.** The v0 audience ships mostly agent-authored code by default. A badge, policy signal, or evidence-bar adjustment based on authorship has no meaningful target here — every PR is roughly "agent-authored" and the reviewer is weighing architecture, not attribution. No UI affordance, no policy branching, no detection heuristics in v0 or v1.

---

## Appendix A · technology seeds

| ID | Why | Source |
|:---|:---|:---|
| R1 | `tree-sitter` incremental parsing | https://tree-sitter.github.io/tree-sitter/ |
| R2 | `tree-sitter-typescript` grammar | https://github.com/tree-sitter/tree-sitter-typescript |
| R3 | `swc` — Rust TypeScript compiler (Next.js / Deno / Turbopack) | https://swc.rs/ |
| R4 | `oxc` — Rust JS/TS toolchain (parser + linter + resolver + experimental checker) | https://oxc.rs/ |
| R5 | `biome` — Rust JS/TS analyzer / linter / formatter | https://biomejs.dev/ |
| R6 | `scip` — source code indexing protocol | https://github.com/sourcegraph/scip |
| R7 | `scip-typescript` — precise index for TS codebases | https://github.com/sourcegraph/scip-typescript |
| R8 | SARIF — interchange format for static-analysis results | https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html |
| R9 | OpenTelemetry traces | https://opentelemetry.io/docs/reference/specification/overview/ |
| R10 | `tokio` — Rust async runtime | https://tokio.rs/ |
| R11 | OPA / Rego — policy-as-code for gating | https://www.openpolicyagent.org/docs/policy-language |
| R12 | SLSA provenance | https://slsa.dev/provenance |
| R13 | Apalache — TLA+ model checker | https://apalache-mc.org/ |
| R14 | Glean — diff sketches, code facts at scale | https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/ |
| R15 | Joern / CPG — code property graphs | https://docs.joern.io/code-property-graph/ |
| R16 | CodeQL — path/data-flow queries | https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/ |
| R17 | Semgrep taint mode | https://semgrep.dev/docs/writing-rules/data-flow/taint-mode/overview |
| R18 | tree-sitter-graph DSL | https://github.com/tree-sitter/tree-sitter-graph |

---

## Appendix B · changes from pre-RFC

- **Tech stack locked** (pre-RFC was undecided; Go/Java was suggested for demonstration quality). Rust on the server, TypeScript in front, TypeScript as first analyzed language, Python ruled out as first target.
- **Audience sharpened.** The original pre-RFC target was reviewers at organizations that already review. The RFC target is AI adopters at the demo-to-production inflection who skip review.
- **Surface shape locked to seven views** (pre-RFC was abstract). Side-by-side flow, intent-vs-result morph, signed drivers-first cost, node-contextual code panel, raw-diff drill-down.
- **Visual language locked.** Inter + JetBrains Mono · neutral near-white surface · slide transitions · color + weight for emphasis · no italic serif.
- **Continuation cost translated to tokens.** The abstract `ctx-tok` unit now has a concrete LLM-token translation. Net number gated by confidence threshold.
- **Calibration repo locked to Inngest.** Chosen because its patterns (retries, durable workflows, state machines, event-driven async) map directly to the strongest v0 hunk types.
- **Proposal sheet format locked to explicit YAML block** (`adr-proposal`) in the PR description, with CLI scaffold to draft from free-text. Pre-RFC left format open.
- **Eval question articulated** in addition to the exit gate: *for which PR classes does the surface beat raw diff, and where doesn't it?* The gate is pass/fail; the question is what the gate measures.
- **Focus mode, agent-packet projection, cold-start harness, TLA+ gating** — all named as v1+ deferrals with explicit reasons.
- **Claim taxonomy model, eval confidentiality protocol, proposal-sheet adoption path** — all resolved with leans. Agent-authored detection dismissed as not meaningful for this audience.
- **Cost model rewritten** after the realization that we are trying to measure LLM cognition with deterministic tools. Drivers are now continuous (not binary fires) and named after the LLM-reading behavior they proxy. Baselines are pinned to `(sha, cli, model, version)`; comparisons refuse when anything drifts. LLM CLIs (claude · codex · gemini · opencode) are integrated, not rebuilt. Validation probes at calibration time; per-PR runtime stays deterministic.
- **v0 is self-hostable only**; hosted SaaS with repo indexing, retained baselines, and model routing is explicitly v1+ work.
