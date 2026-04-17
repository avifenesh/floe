---
title: "Spike plan · v0 · architecture delta review"
date: 2026-04-17
status: Active
context: Solo developer · self-hostable product · testers-only rollout · no team, no meetings
companion: architecture_delta_review_rfc.md
timeline: 10–12 weeks
---

# Spike plan · v0

Six scopes of two weeks each. Each scope ends with a demoable acceptance gate; failing a gate is a signal to stop or narrow, not to extend the scope.

## Solo-developer posture

This is a one-person build. Every place the generic plan would say "team decision" or "pair review" is restructured to either **written rubric + one-week re-check** (for subjective calls) or **external reviewer** (for anything that benefits from a second mind). Recruitment, labeling, and the final decision are all solo tasks. Accept the tradeoffs:

- **no dissenting voice in the room** · mitigate with pre-committed kill criteria (in the RFC; see scope 6) and written rubrics you sign yourself at scope 4 and don't renegotiate later
- **labeling is subjective** · mitigate with a written rubric (`docs/labeling-rubric.md`) and a one-week re-labeling pass to check internal consistency
- **recruitment is slow** · start outreach in scope 3, not scope 5
- **scope-creep pressure is personal** · the RFC's v1-narrower-than-v0 rule is the only anti-creep discipline; treat it as non-negotiable
- **decision fatigue** · schedule the scope 6 decision on a specific day in week 11, written cold before re-reading your own advocacy

## At-a-glance

| # | Scope | Weeks | Acceptance demo |
|:---|:---|:---|:---|
| 1 | Graph + schema + three hunks | 1–2 | `adr diff` emits JSON with call · state · api hunks on a fixture PR |
| 2 | CFG + state-machine detection · orchestrator · frontend scaffold | 3–4 | `bin/demo.sh` runs server, SSE progresses, frontend loads text-only artifact |
| 3 | Seven views come alive | 5–6 | Real Inngest PR renders all seven views with real pipeline data |
| 4 | Evidence + cost model · baseline · LLM CLI | 7–8 | Held-out PRs render full cost + claim data; calibration report committed |
| 5 | Real eval with 8 known-figure reviewers | 9–10 | `eval-report.md` committed with experience + time as primary metrics |
| 6 | Narrow or kill | 11–12 | One of {kill memo · narrowed scope · v1 RFC} signed and committed |

## Cross-cutting

- **Cargo workspace**: `adr-core` · `adr-parse` · `adr-hunks` · `adr-cfg` · `adr-state` · `adr-evidence` · `adr-cost` · `adr-llm` · `adr-eval` · `adr-server` · `adr-cli`
- **Frontend**: Vite + React + Tailwind + shadcn/ui in `apps/web`
- **Types**: Rust → TS via `ts-rs` derive; never hand-maintain a parallel type def
- **Testing**: `cargo test` + `insta` snapshots on JSON artifacts; fixture PRs as golden inputs from scope 1 onward
- **Kill gates** (RFC): experience ≥ 4/5 median · time −25% on target classes · seeded-bug catch-rate strictly higher than raw diff

---

## Scope 1 · graph + schema + three hunks (weeks 1–2)

**goal** · emit a deterministic versioned JSON artifact for a real TypeScript PR, containing three working semantic hunks. No UI, no LLM, no cost model — just the machine-readable substrate everything downstream reads from.

**deliverables**
1. Cargo workspace (`adr-core` · `adr-parse` · `adr-hunks` · `adr-cli`).
2. Graph schema v0.1 — versioned, JSON-serializable. Nodes (function · type · state · api-endpoint · file), edges (calls · defines · exports · transitions), every node/edge carrying `provenance: { source, version, pass_id, hash }`.
3. Three hunk extractors end-to-end: **call** · **state** (classical `type State = "a" | "b"` idiom only) · **api** (exported types + Next.js route handler signatures).
4. Fixture corpus · 4–5 handwritten tiny PRs exercising each hunk type, committed as golden inputs.
5. Snapshot tests on JSON output per fixture PR.

**decisions locked inside scope**
- Official `tree-sitter` crate
- Shell out to `scip-typescript` once at indexing, parse `.scip` protobuf in Rust
- `petgraph` for queries, serialized as flat adjacency lists
- `schemars` for JSON schema derivation
- Schema versioning in frontmatter, bump minor on breaking changes until v1

**acceptance** · `cargo run -p adr-cli -- diff fixtures/pr-0284-base fixtures/pr-0284-head > out.json` emits an artifact with ≥ 1 node and ≥ 1 edge per hunk type, fully provenanced. Run live against a fixture, eyeball the JSON.

**risks**
- scip-typescript version drift on Inngest · pin it
- state extractor overfits to one idiom · document the recall gap, accept it for v0
- schema churn · v0.1 is allowed to be rough; bump at scope 3

**out-edges** · stable JSON format for the orchestrator · call graph deltas as CFG input · fixture corpus reusable by scope 4 calibration

---

## Scope 2 · CFG + state-machine detection · orchestrator · frontend scaffold (weeks 3–4)

**goal** · prove the three-piece runtime works end-to-end. Rust service accepts a PR reference, produces the scope-1 artifact augmented with CFG and state-machine facts, TS frontend fetches and renders stubs of the seven views. All text, no visuals — scope 3 does visuals.

**deliverables**
1. `adr-cfg` · per-function CFG on tree-sitter AST. Nodes: `seq · branch · loop · async-boundary · throw · try · return`. Async/await produces explicit yield points. Merged into artifact under `cfg: { ... }`.
2. `adr-state` · classical SM detection only (field + string-union + assignment transitions). Inngest `step.run` pattern and xstate deferred to scope 4 only if signal.
3. `adr-server` · axum: `POST /analyze` (returns jobId), `GET /analyze/:jobId` (pending/ready/error), `GET /analyze/:jobId/stream` (SSE progress). Single worker, sled cache by (repo, sha).
4. `apps/web` · Vite + React. Seven route shells, slide transition primitive, spine header, palette stub. `adr-schema-ts/` generated by `ts-rs`. Single `useArtifact(jobId)` hook via React Query.
5. `bin/demo.sh` · starts server, triggers analysis on fixture corpus, opens frontend, confirms the artifact renders (as text).

**decisions locked inside scope**
- axum on tokio · ts-rs for types · sled for cache · SSE for progress
- plain custom hash routing · shadcn Tabs pattern for spine
- **Vite + React + Tailwind + shadcn/ui** (locked earlier)

**acceptance** · live run of `bin/demo.sh fixtures/pr-0284`: server starts, SSE ticks through `scip → cfg → hunks`, frontend at `localhost:5173` navigates all seven stub views with slide transitions, refresh hits cache in < 1s.

**risks**
- scip-typescript cold indexing is 30–90s on Inngest · pre-warm the cache for fixtures in the demo script
- CFG is not SSA · document precision gap, don't chase SSA
- ts-rs enum encoding drifts between Rust serde and TS · pick `#[serde(tag = "type")]` convention once, keep it
- scope creep into visuals · resist, scope 3 is the visual scope

**out-edges** · fully-typed artifact object for the frontend · working spine + slide primitive · orchestrator cached and fast enough for live iteration

---

## Scope 3 · seven views come alive (weeks 5–6)

**goal** · the artifact from scope 2 turns into the real product surface. Every view renders real pipeline data, every node is clickable, every shortcut works. A real Inngest PR demoed at end of week 6.

**deliverables**
1. Component library · shadcn base (Sheet · Command · Tooltip · Dialog · Skeleton · Toast · Card) + custom (`FlowCanvas` · `NavArrow` · `CostAxis` · `ClaimRow` · `DeltaCard` · `MorphRow` · `ReplacementRow` · `DiffLine` · `NodePanel` · `Spine`).
2. All seven views wired to artifact:
   - **01 pr** · stats · hunks · proposal chips · provenance
   - **02 flow** · two `FlowCanvas` with packet animation via `getPointAtLength`
   - **03 morph** · `intent[]` + `replacements[]` rows
   - **04 delta** · `deltas[]` cards with per-node drill
   - **05 evidence** · `claims[]` with strength + provenance
   - **06 cost** · `axes[]` drivers-first, net gated by confidence ≥ 0.70
   - **07 source** · file tabs + side-by-side `DiffLine` with gutter
3. Cross-cutting · Framer Motion direction-aware slides · Sheet contextual panel (non-modal) on node click · Command palette on `/` · fixed-edge `NavArrow` (grow on hover) · keyboard nav.
4. Skeleton + error states via SSE progress; toast on errors.
5. One artifact-driven state hook · views are pure functions of it.

**decisions locked inside scope**
- Framer Motion + AnimatePresence for view transitions
- React Query (retries, cache, SSE integration)
- Custom diff renderer (artifact is already tokenized)
- lucide-react icons
- Light mode only; dark mode deferred
- Accessibility baseline: keyboard reachable, AA contrast, Sheet non-modal

**acceptance** · open browser on a real Inngest PR artifact, every view renders with real data, flow view plays both packets (head exits visibly before base's first sleep), any node click opens Sheet with code + 4 cost pills + touching claims, `/` palette filters and navigates, keyboard `←/→` + nav-arrow transitions are direction-correct, source view renders gutter diff correctly. 3-minute screen recording.

**risks**
- flow canvas visual regression from `flow.html` sketch · reserve 2 days at end of week 6 for per-PR layout tuning
- Framer Motion bundle ~40 KB · acceptable
- Sheet modal overlay · customize `<SheetContent modal={false}>`
- large diff stutters · virtualize `DiffLine` if > 500 lines
- SSE reconnection on network drop · React Query + explicit retry; document as known v0 limitation

**out-edges** · working frontend on a fully-typed artifact · `NodePanel` accepts cost/evidence/code fields that scope 4 populates with real data · spine + palette ready for v1 `packet` projection toggle

---

## Scope 4 · evidence + cost model · baseline · LLM CLI (weeks 7–8)

**goal** · claims and cost drivers stop being hand-authored fixtures. Deterministic drivers grounded in LLM-navigation research. Baselines pinned per `(sha, cli, model, version)`. LLM CLIs integrated, not rebuilt.

**deliverables**
1. `adr-evidence` · four collectors (**PERF** · **DATA** · **API** · **LOCK**). Each returns `Evidence { class, strength, provenance }`. See RFC § 7 for minimums.
2. `adr-cost` · driver catalog (continuous scores, not binary):
   - `grep_friendliness` · `file_read_cost` · `scopes_added` · `logical_steps` · `retrieval_ambiguity` · `docs_alignment`
   - coefficients in `coefficients.toml` fit per-repo against LLM-probe ground truth
3. `adr baseline` · pins `(commit_sha, llm_tool, llm_model, llm_version)`, persists to `.adr/baseline-<sha>.json`. Any mismatch on subsequent run → UI refuses delta, surfaces "re-baseline required".
4. `adr-llm` · adapters for `claude -p` · `codex exec -p` · `gemini` · `opencode run`. `$PATH` + `--version` detection. `.adr/llm.toml` config. Graceful degrade if nothing installed (`confidence: unknown`).
5. Validation probe (opt-in) · `adr baseline --propose-questions` runs LLM over repo once, proposes 3 follow-up tasks. Opt-out uses 3 generic prompts per detected codebase type (classifier over `package.json`, `tsconfig.json`, file structure).
6. Calibration set · `fixtures/inngest-24/` · 24 historical Inngest PRs with hand-labeled `expected.toml`. **Labeled solo, with written rubric (`docs/labeling-rubric.md`), with one-week re-label pass for internal consistency.** Document disagreements with yourself as low-confidence coefficients.
7. `adr calibrate` · fits linear coefficients minimizing signed-direction + magnitude mismatch on the 24 set. Emits `coefficients.toml` + `calibration-report.md`.
8. Orchestrator integration · stages `scip → hunks → cfg → state → evidence[4] → cost`. Each stage streams SSE progress. End-to-end < 90s warm.
9. Cost-view strip · derived string, not hand-authored: `"structural approximation of LLM continuation cost; calibrated against 24 real LLM sessions in inngest; residual σ = N tokens."`

**decisions locked inside scope**
- Bench · shell out to repo's own script; don't reinvent a runner
- OTEL · in-process, file sink
- TS compiler API · long-lived Node sidecar over stdio JSON-RPC
- Per-hunk-class confidence bands (not global)
- LLM probe cost during calibration · ~$30–80 one-time on whichever CLI you have installed
- No evidence fabrication · missing evidence = `strength: none`

**acceptance**
1. Full pipeline on three held-out Inngest PRs. Every claim has real `strength` + `provenance`. Every cost axis has drivers + coefficients.
2. On 24-PR calibration set · driver prediction matches LLM-probe sign ≥ 20/24 per axis · magnitude within 1σ on ≥ 16/24.
3. CLI mismatch produces legible `re-baseline required` error.
4. `calibration-report.md` committed; every miss diagnosed honestly.
5. Held-out PR review by the solo dev on a second day confirms drivers feel plausible.

**risks**
- Inngest has thin bench coverage · many PERF claims will be `strength: none`; document honestly, don't fabricate
- 24 PRs is thin · coefficients will feel approximate; scope 5 eval is the real confidence test
- Solo labeling is subjective · rubric + re-label is the mitigation; accept the honesty tax
- TS compiler drift · pin the sidecar TS version, rerun calibration if bumped, record in provenance

**out-edges** · every UI field is now derived · `calibration-report.md` is scope 5's baseline for debating cost scores · held-out set (3 here, 10 in scope 5) stays disjoint from calibration

---

## Scope 5 · the real eval (weeks 9–10)

**goal** · run the measurement protocol. Produce a signed, honest answer to: *for which TypeScript PR classes does the v0 surface let a reviewer reach a correct verdict faster, with more positive experience, than raw-diff review?*

**what we're selling** (calibrates the metric design) · **not** "you'll reach correct verdicts" — table stakes, every careful reviewer does that. **Yes** "easier and more correct" — ease is leading, correctness is lagging validation.

**deliverables**
1. **Eval corpus** · 10 held-out Inngest PRs (disjoint from calibration; stratified across hunk classes) + 3 seeded *architectural* bugs on a private Inngest fork. Seeded bugs must pass raw-diff review silently — only v0 surfaces them. Candidates:
   - **silent invariant violation** · detached async publish (`void machine.run()`) that leaks on process exit
   - **proposal drift** · proposal sheet declares 4 states, implementation adds a 5th unclaimed (circuit breaker)
   - **cross-hunk contract break** · API widens one place (+409), consumer in another place doesn't handle 409
2. **Reviewer recruitment** — 8 known-figure TS / AI-builder reviewers from Twitter/X. Outreach started in scope 3, confirmed by end of scope 4. Direct DM, brief pitch, paid slot + early access to spike output. Aim for 5–50k follower builders who *ship and write*, not the biggest accounts. Include at least 2 Inngest-adjacent folks.
3. **Pricing** · **$100/reviewer for 2 PRs** baseline. **$300/reviewer** only if deliberately large (few-K-line) PR. Budget $1.2–1.4k total.
4. `adr-eval` crate · serves precomputed artifacts + review form · auto-captures time · stores `eval-run-<ts>/*.jsonl` · produces `eval-report.md` with per-PR × reviewer × mode breakdowns.
5. **Metrics collected** (primary → secondary):
   - *primary · experience* · post-review Likert on *positive · choose-next-time · helped-see · fought-ui*; free-text *surprised-me · got-in-my-way*; NPS-style *would-recommend*
   - *primary · time* · time-to-direction-verdict, time-to-complete, time-to-catch on seeded bugs
   - *secondary · correctness* · seeded-bug catch rate, verdict vs. expert-panel ground truth
6. **Expert panel** · 2 senior TS / distributed-systems reviewers independent of the product, recruited separately, compensated, providing ground-truth verdict. Consensus = ground truth.
7. **Onboarding video** · 2 min, recorded once, shared with each reviewer. One day of week 9.

**decisions locked inside scope**
- Raw-diff baseline is GitHub's default split-diff, no annotations · fair "what they use today" comparison
- Reviewer order randomized · mode order rotated per reviewer · 1-week separation between modes per reviewer
- Seeded bugs randomized within the 13-PR sequence (not clustered)
- "Preferred" = Likert ≥ 4 on choose-next-time

**acceptance**
1. 13 PRs · 16 reviews (8 reviewers × 2 PRs) + expert-panel ground truth completed. No dropouts. No corrupted logs.
2. `eval-report.md` committed with experience + time as primary, correctness as secondary. No cherry-picking.
3. Seeded architectural bugs: raw diff catches ≤ 1/3 on average; v0 catches ≥ 2/3 on average.
4. Reviewer NPS-style median ≥ 4/5 for v0.
5. At least 2 reviewers post publicly about the experience (not required, but a signal).

**risks**
- Recruitment drags · start in scope 3, confirm by end of scope 4
- Hawthorne effect · affects both modes symmetrically, tolerable
- 13 PRs is thin · acknowledge statistical weakness; don't over-claim
- Trivial held-out PRs · weight sampling toward non-trivial diffs from recent months
- Solo eval runner is the dev who built the product · mitigate with written review-form that can't be post-hoc edited; store `eval-run-*/*.jsonl` in git to prevent tampering

**out-edges** · flagged regressions list for scope 6 · sorted "where raw diff won" list (explicit non-coverage in v1) · `eval-report.md` becomes the only document debated in scope 6

---

## Scope 6 · narrow or kill (weeks 11–12)

**goal** · convert eval signal into an irreversible decision. No new features. At end of scope 6, one of three things is in the repo: kill memo, narrowed scope, or v1 RFC. Signed (committed) and public.

**deliverables**
1. **Signal triage** (week 11, days 1–3) · read `eval-report.md` cold, tag reviewer free-text against pre-committed codes (helped · confused · missed · praised · fought-ui · bored), re-read cost-model regressions, collect "what I didn't expect" list (usually the densest signal).
2. **Decision** (week 11, day 4) · *not a meeting*. Written decision, 3-hour block. Strict order: (a) exit-gate status per leg, (b) narrow / kill / proceed posture, (c) specific narrowing if narrow, (d) public-comms plan. Commit `decisions/spike-outcome.md` — one page, dated — before the block ends.
3. **One of three artifacts** committed by end of week 11:
   - **kill** · `decisions/spike-killed.md` · honest post-mortem. Thesis, what eval showed, why stopped. Read-only repo. Open-source the code if no competitive liability.
   - **narrow** · `decisions/spike-narrowed.md` + updated v1 scope · names exactly which views / hunks / claim classes / PR classes are in v0.5 or v1.
   - **proceed** · `architecture_delta_review_v1_rfc.md` drafted. Always *more specific* than v0 RFC, never broader.
4. **Public write-up** (week 12, days 1–3) · blog + thread + GitHub README update. Committed to reviewers in scope 5. Honest, not promotional. Builders respect the honesty; overclaiming burns trust.
5. **Reviewer debrief** (week 12, day 4) · 30-min async (preferred) with all 8 reviewers. Share: findings · decision · v1 invitation (if proceeding).
6. **Codebase disposition** (week 12, day 5) · tag `v0.1-spike-end` · close spike-only issues · if proceeding, clean branch for v1 work. Scope-1 schema and scope-4 cost model carry forward; UI is likely a partial reset.

**decisions locked inside scope**
- Narrow threshold · 1 of 3 exit-gate legs passes = narrow · 2 of 3 = proceed but tighten · 3 of 3 = proceed clean · 0 = kill
- **Kill trigger · if only correctness passes and experience is flat, it's kill, not narrow.** We sold ease. If ease isn't there, the product isn't.
- Open-sourcing default · yes on kill · deferred on proceed (preserve competitive position during early v1)
- v1 RFC constitution · **narrower than v0, never broader. One axis of expansion at most.** Non-negotiable.

**acceptance**
1. Decision memo committed, signed (self), dated.
2. One of {kill memo · narrowed scope · v1 RFC} in the repo as a markdown file.
3. Public write-up published.
4. 8 reviewers debriefed.
5. Codebase in its end-state: archived, branched, or tagged.
6. Zero open "what do we do now" questions.

**risks**
- Mixed-signal paralysis · resist in-scope-6 iteration. The spike is done; iteration is in v1 or not at all.
- Solo attachment after 10 weeks of building · **pre-commit the kill criteria in scope 1** (already in RFC); treat them as mechanical rules, not renegotiable judgments
- Reviewer fallout if killed · payment is for the review, not the outcome · write-up is honest · most builders will respect it more than a spin
- v1 scope creep disguised as "proceed" · the v1-narrower-than-v0 rule is a one-line enforcement, no exceptions
- **Solo bias in decision-making** · write the decision cold before re-reading your own advocacy in scope 3–5 notes · if possible, invite one external reviewer from scope 5 to read the decision memo before you commit it (they can veto or confirm; not edit)

**out-edges**
- Kill · none, spike is terminal
- Narrow or proceed · v1 RFC is the next document; this RFC ends with scope 6

---

## Addenda · solo-specific discipline

- **Rubric-first labeling** · any subjective call (calibration labels in scope 4, reviewer free-text coding in scope 5 + 6) uses a written rubric committed *before* the data lands. Re-labeling a week later with the same rubric + comparing internal-disagreement rate is the only sanity check available solo.
- **External checkpoints** · end of scope 2, 4, and 5 each have a 15-min loom video shared with 1–2 trusted builders. Not for decisions, just for "does this look reasonable". Frictionless, no expectation of feedback.
- **Timeboxing** · each scope is 2 weeks hard. If scope 1 slips into week 3, the missing work gets deleted, not extended. The plan fails honestly, not by fake progress.
- **Public commit cadence** · weekly public commits to the repo with a short CHANGELOG entry. Accountability without ceremony.
- **Decision pre-commits** · kill criteria signed at scope 1 · narrow-vs-kill rule signed at scope 4 · no re-signing allowed after the eval lands.
