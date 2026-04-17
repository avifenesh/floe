---
title: "Pre-RFC: Semantic architecture review for agent-authored PRs"
date: 2026-04-17
status: Pre-RFC / exploratory
decision_requested: Approve a research spike and a thin prototype
primary_review_target: Architecture direction, implementation, and cost delta
out_of_scope: Naming freeze, final schema, full language coverage
---

# Pre-RFC: Semantic architecture review for agent-authored PRs

*Working title · Draft for internal discussion · April 17, 2026*

---

## Executive summary

- Today’s PR review flow asks humans to reconstruct architecture from raw line diffs. That model breaks down once agents can emit thousands of lines quickly and syntax-level mistakes are mostly caught by CI or bots.
- The problem to solve is not “review the agent’s intent.” The problem is to review whether the proposed architectural direction is right, whether the implementation actually realizes that direction, and whether the change improves or worsens long-term cost.
- This pre-RFC explores a new review model: a visual architecture diff backed by machine-readable artifacts for proposed direction, observed architecture delta, claim support, and signed cost delta.
- The agent’s proposal is just input. Human reviewers remain the authority on architectural direction. The system should help them spend attention on direction, invariants, evidence, and future cost rather than on reconstructing structure from line-by-line text.

> **Working framing:** The system is not an agent-intent reviewer. It is an architecture delta reviewer. The core questions are: Is the proposed architectural direction right? Did the implementation actually realize that direction? What guarantees changed? What evidence exists? What cost delta did the change introduce?

---

## Problem worth solving

The existing pull-request model is optimized for human-typed code and textual deltas. That model assumes the reviewer’s scarce skill is catching syntax mistakes, local bugs, or style issues that appear directly in lines of code. For large agent-written changes, that assumption is backwards. Syntax errors are often caught cheaply by CI or bots. What remains expensive—and deeply human—is deciding whether the architecture is moving in the right direction, whether the implementation really matches that direction, and what long-term cost has been created or removed.

| Pain | Why current review underperforms | What the new review system must provide |
|:---|:---|:---|
| Line-diff overload | Reviewers are forced to infer execution flow and state changes from textual hunks. Large agent-written PRs make that reconstruction slow and fragile. | Make the review unit a semantic hunk and render architecture before/after, not just text before/after. |
| Wrong direction can still look well implemented | A change can perfectly match the agent’s own story and still be the wrong architectural move for the codebase. | Separate direction review from implementation review. Proposal quality and implementation quality must be judged independently. |
| Claims arrive without proof | Agents often claim performance, safety, or simplification benefits while only providing happy-path tests. | Require claim-specific evidence: benchmarks for performance, state/lock proofs for concurrency, path evidence for data flow, docs alignment for maintainability. |
| Future continuation cost is invisible | A PR can pass tests yet scatter logic, create hidden invariants, or increase prompt/API token cost for every future session. | Report a signed cost delta that can go positive or negative, with drivers and confidence—not a fake universal score. |
| Docs and cleanup do not get credit | Refactors, docs sync, and deletion of hallucinated abstractions often improve long-term maintainability but look noisy in raw diffs. | Reward negative cost deltas when the change genuinely simplifies future work. |
| Review assistance is not trustworthy enough | If the same agent writes the code, explains the code, and rates the code, the workflow just mechanizes self-certification. | Separate proposal, observation, evidence, policy, and human decision into different trust classes. |

---

## Desired outcomes

- Let humans review architectural direction instead of manually reconstructing architecture from line diffs.
- Show which architectural structures were added, removed, simplified, or riskily altered.
- Expose invariant changes explicitly: state transitions, lock/resource ordering, publication points, error paths, and cross-boundary data flow.
- Make proof debt visible. If a claim lacks the right evidence, the UI should say so directly.
- Measure whether the change makes the next session easier or harder, and allow that delta to be negative when the codebase is simplified.
- Use one low-level representation that supports both human review and future agent review.

---

## Explicit non-goals

- Replace Git, PR systems, CI, or existing code hosts.
- Eliminate raw diff review entirely; raw diff remains a drill-down, not the primary review surface.
- Let the generator agent define what the right architectural direction is.
- Pretend there is one repo-agnostic, universal complexity score that is meaningful everywhere.
- Require full formal proof for every change or block all progress behind verification tooling.
- Support every language or every kind of architecture delta in v0.

---

## Working principles

| Principle | Detail |
|:---|:---|
| **Proposal ≠ accepted direction** | The agent or author may propose a direction, but the system must treat that proposal as untrusted input until a human accepts it. |
| **Direction, implementation, evidence, and cost are separate** | A PR may be directionally wrong but well implemented, directionally right but under-proven, or a net simplification that still weakens a critical invariant. |
| **Cost deltas are signed** | Refactors, cleanup, docs sync, and deletion of hallucinated abstractions can reduce future work. The model must represent negative deltas cleanly. |
| **Measurements are repo-relative** | Continuation cost should be compared against base for a flow and task class, not reported as an absolute universal truth. |
| **Red/green is preserved** | Humans already understand old-vs-new via red removed and green added. Semantic diff should keep that grammar and layer additional signals on top of it. |
| **One graph, multiple projections** | The same underlying representation should power human-facing visuals, PR annotations, policy evaluation, and agent review packets. |

---

## What the system should produce

The first mistake to avoid is overloading one artifact with every job. The review pipeline should instead produce a small set of linked artifacts, each with a different consumer and trust level.

| Artifact | Primary consumer | Produced by | Purpose |
|:---|:---|:---|:---|
| Proposed direction sheet | Human reviewer | Agent / author / issue / design doc normalizer | Summarizes the proposed architectural move, stated benefits, non-goals, and explicit claims. |
| Observed architecture diff | Human reviewer, policy engine, auditor agent | Parser / index / analyzer pipeline | Describes what actually changed in control flow, data flow, state transitions, resource flow, and public surface area. |
| Claim support map | Human reviewer, policy engine | CI, analyzers, benchmark harnesses, traces, formal tools | Maps each claim to evidence, counter-evidence, strength, and missing proofs. |
| Architecture cost delta | Human reviewer, policy engine | Cost model + empirical benchmarks | Summarizes signed deltas across continuation cost, runtime cost, operational cost, and proof burden. |
| Visual flow diff | Human reviewer | Renderer over semantic hunks | Presents old-vs-new architecture in a red/green visual diff with evidence and cost overlays. |
| Agent review packet | Auditor agent, future sessions | Same pipeline as above | Provides stable graph facts, minimal context packs, and review queries for machine consumers. |

---

## Trust model and data providers

Every field in the system should be attributable to a provider. Proposal text and claimed benefits may originate from the agent or author, but observed deltas, proof, costs, and policy decisions should come from separate providers. This separation prevents the workflow from becoming self-certification wrapped in nicer visuals.

| Field / artifact | Primary provider | Trust class | Notes |
|:---|:---|:---|:---|
| Proposed direction / claims | Agent, human author, issue, design doc | Declared / untrusted | Useful starting point; never treated as ground truth. |
| Changed files, symbols, call sites | Git + parser + code index | Observed | Basic structural fact set. |
| Observed architecture delta | Static analyzers / graph builders / language tooling | Derived | Must be attributable to tooling and versioned analysis passes. |
| Claim support | CI, test harnesses, profilers, trace systems, verification tools | Empirical or derived | Each evidence item should record strength and provenance. |
| Review prompts | Generated from templates + risk rules + detected semantic hunks | Advisory / generated | These are machine-generated reviewer prompts, not authoritative questions supplied by the agent. |
| Human review decision | Assigned reviewer(s) | Authoritative judgment | Direction approval, override, or rejection remains a human act. |

> Review prompts should be **generated**, not hand-written by the agent. A good system can derive reviewer prompts from semantic hunk type, risk rules, and detected contradictions. The reviewer can ignore, edit, or add to these prompts, but the system should create them automatically.

---

## Architecture cost delta

Cost should not collapse into one brittle score. The review target here is architecture cost delta: the change in future burden introduced by a PR. That burden has multiple axes. Some axes can go down after a cleanup or refactor, so the model must support negative deltas explicitly.

| Cost axis | Definition |
|:---|:---|
| Continuation cost | How much context and reconstruction effort the next human or agent session will need to safely continue work on the affected flow. |
| Runtime cost | Latency, throughput, memory, CPU, I/O, or tail-behavior effects introduced by the change. |
| Operational cost | Oncall burden, deployment complexity, observability requirements, failure blast radius, and rollback complexity. |
| Proof cost | How much new evidence, model maintenance, or invariant checking is now required to justify the system’s behavior. |

- Signed delta is mandatory. Refactors, cleanup, docs sync, dead-code removal, and deletion of hallucinated abstractions must be able to score as improvements.
- Continuation cost should be repo-relative and task-class-relative.
- Any reported number must include confidence and drivers. The system should explain why cost moved, not merely state that it moved.

---

## Research work required before a full RFC

The next step is not a full product commitment. The next step is disciplined research across a small number of workstreams, each with a clear exit criterion. If these workstreams do not converge, the project should not move to a full RFC.

| Workstream | Key questions | Likely starting points | Exit criterion |
|:---|:---|:---|:---|
| 1. Architecture delta representation | What are the stable nodes, edges, and semantic hunk types? How do we preserve identity across base/head and support both human and agent consumers? | Tree-sitter, SCIP/LSIF, Glean diff sketches, code property graphs, Kythe / stack-graphs / tree-sitter-graph as design references. | A versioned graph schema and at least one concrete example that can render a meaningful architecture diff for a real PR. |
| 2. Semantic extraction | Which architectural facts can be extracted reliably for the first target language: call flow, state transitions, lock/resource flow, error paths, public API deltas, docs-code links? | Tree-sitter, language-native analyzers, CodeQL path/data-flow, Semgrep taint, Joern/CPG, compiler outputs where available. | Measured precision/recall on a seed set of changes that matter to reviewers. |
| 3. Direction vs implementation split | How do we represent a proposal without granting it authority? How do we show partial matches, contradictions, or implementation drift from the proposed move? | Structured claim objects, design-doc normalization, mismatch taxonomy. | A review surface where ‘wrong direction, correct implementation’ and ‘right direction, wrong implementation’ are both visible states. |
| 4. Evidence and proof model | What claim classes exist and what evidence is acceptable for each? What strength model is legible to humans? | SARIF, benchmark harnesses, traces, TLA+/Apalache, CBMC, KLEE, Dafny, CodeQL, Semgrep, existing CI/test outputs. | A claim support map with per-claim minimum evidence rules and counter-evidence handling. |
| 5. Signed cost delta model | How do we estimate future work cost without pretending to know the repo perfectly? How do we reward cleanup and docs sync? | Static base-vs-head estimation, repo-calibrated models, cold-start continuation tests, docs alignment scoring, redundancy cleanup detection. | Signed, explainable deltas with drivers, confidence, and no single fake universal number. |
| 6. Visual UX / Flow Diff | What should the primary review screen show? How do we keep red/green diff intuition while presenting semantic structure? | Side-by-side semantic diff, overlay diff, semantic hunk cards, evidence badges, cost heat, raw diff drill-down, design prototypes. | A prototype that reviewers can use on real PRs and prefer over raw diff for at least some review tasks. |
| 7. Integration surface | Is the first product a standalone review app, a Theia-based shell, a VS Code extension, or a code-host integration? | Theia architecture and extension mechanisms, VS Code webviews/views, code-host annotation surfaces. | A clear first substrate for prototype and a rationale for why it is the right scope boundary. |
| 8. Evaluation strategy | How do we know the system is better? What benchmark tasks and historical PRs should be used? How do we seed realistic failure cases? | Historical internal PRs, seeded concurrency/perf bugs, reviewer timing, disagreement analysis, cold-start continuation tests. | A baseline and a clear win/loss criterion before a full RFC. |
| 9. Governance, provenance, and policy | Who signs artifacts? What gets stored? What can block merges? How do overrides work? | SLSA provenance patterns, OPA/Rego policy-as-code, team review policy, retention/privacy constraints. | A governance model that is auditable and practical. |
| 10. Language and repo strategy | Which language first? Build-dependent or build-light? Single repo or multi-repo? Generated code handling? | One target language first (likely Go or Java), repo profiles, exclude/vendor/generated code strategy. | A narrow v0 scope with a credible path to broaden later. |

---

## Candidate technical direction

A likely technical direction is to layer a new architecture-review plane on top of existing repository, analysis, and CI infrastructure rather than replacing those systems. The differentiator is not a new parser protocol; it is the combination of semantic architecture diff, evidence model, and signed cost delta.

- Treat the new system as an architecture-review plane layered on top of Git/CI, not as a replacement for them.
- Prototype the machine-readable core first: proposed direction sheet, observed architecture diff, claim support map, and signed cost delta.
- Build the first product around semantic hunks rather than whole-program graphs. A semantic hunk is the architecture equivalent of a diff hunk.
- Start with a narrow language and claim set: for example, call/control flow, lock/resource flow, basic state transitions, API surface changes, and performance claims.
- Keep raw diff, logs, tests, and traces linked, but subordinate them to the architectural review surface.

| Layer | Main output | Likely starting points |
|:---|:---|:---|
| Source / index layer | Git, changed files, parser outputs, precise code indexes | Tree-sitter [R1], SCIP [R2], LSIF [R3] |
| Semantic analysis layer | Observed architecture delta, semantic hunk extraction, mismatch detection | Glean diff sketches [R4], Joern / CPG [R5], CodeQL [R16], Semgrep [R17] |
| Evidence layer | Static findings, test results, benchmarks, traces, model/proof artifacts | SARIF [R6], OpenTelemetry [R7], Apalache [R13], CBMC [R14], KLEE [R15], Dafny [R18] |
| Policy / provenance layer | Signing, gating, overrides, merge rules | SLSA provenance [R12], OPA / Rego [R11] |
| Review surface | Visual Flow Diff, semantic hunk cards, raw drill-down, agent review packets | Standalone web app or custom IDE shell |

---

## Integration options

The integration substrate should be chosen for prototype velocity, not prestige. A good first platform is the one that lets the team test the review model on real pull requests with minimal platform risk.

| Option | Why it is attractive | Main drawback |
|:---|:---|:---|
| Standalone web review app | Fastest route to a purpose-built experience; easiest to share outside one editor; can aggregate CI artifacts and deep visuals. | Requires separate workflow integration and identity/permissions work. |
| Theia-based shell | Best if the long-term product is a custom review-first IDE experience; Theia supports separate frontend/backend processes and compile-time extensions with deep access to internals [R8][R9]. | Heavier investment than a thin prototype. |
| VS Code extension | Good for early adoption and quick reviewer access; webviews can render custom visualizations [R10]. | Webviews should be used sparingly and are resource-heavy [R10]; extension APIs are not ideal as the primary product boundary. |
| Code-host annotations only | Lowest friction entry point for alerts and links back into the review app. | Too cramped for the main experience; better as an integration layer than the core UI. |

---

## Visual direction: the Flow Diff experience

The main screen should not be a line diff with a graph panel bolted on. It should be a semantic review cockpit. At the same time, the visual grammar should preserve what human reviewers already understand from traditional diffing: red removed, green added, gray unchanged context.

- **Primary review unit:** semantic hunk, not file and not whole-program graph.
- **Primary visual grammar:** red removed architecture, green added architecture, gray unchanged context. Yellow/orange can mark uncertainty or partial match. Evidence and cost overlays should not replace red/green.
- **Default mode:** side-by-side semantic diff. Secondary modes: overlay diff and narrative playback for large refactors.
- **Each semantic hunk card** should answer five questions: what changed, what direction was proposed, how well implementation matches the proposal, what evidence exists, and what signed cost delta was introduced.
- The same hunk must also expose stable node IDs, code locations, related docs/tests/artifacts, and minimal context packs for agent consumers.

| Requirement | Implication for the UI |
|:---|:---|
| Red/green old-vs-new stays primary | Semantic diff must still feel like a diff, not like a dashboard or abstract graph toy. |
| Beautiful, but not decorative | The UI should compress meaning, not merely look modern. Avoid graph hairballs and over-dense control surfaces. |
| One graph powers humans and agents | Every visible item should map to a stable machine node or hunk ID with provenance and drill-down. |
| Raw diff remains available | Every semantic hunk must link back to the exact code and artifacts that justify it. |

---

## Measuring continuation cost without pretending to know every repo

A useful cost model can begin before the system deeply knows a repository, but it must be honest about confidence. The right approach is to compare base versus head for an affected flow and task class, then improve that estimate over time as the system accumulates repo-specific data.

- Architecture cost should be multi-axis. Continuation cost is central, but runtime, operational, and proof costs also matter.
- Continuation cost should be repo-relative and task-class-relative: for example “modify cache refresh behavior” or “extend request auth path”.
- The system should support three confidence modes: first-run static estimate, repo-calibrated estimate, and empirical cold-start continuation test.
- Signed delta is mandatory. Cleanup, refactor, dead-code removal, docs sync, and deletion of hallucinated abstractions must be able to score as improvements.

| Mode | How it works | Confidence |
|:---|:---|:---|
| Static first-run estimate | Base-vs-head structural estimate using affected files, semantic dispersion, hidden invariants, docs alignment, and retrieval ambiguity. | Low to medium |
| Repo-calibrated estimate | Learns from historical review objects, recurring flows, past reviewer navigation, and prior continuation tests. | Medium to high |
| Cold-start continuation test | Runs a fresh bounded task on the changed flow with a clean session and measures what it takes to continue safely. | High for the tested task class |

### Cold-start continuation test

- A cold-start continuation test should simulate the next session, not the current authoring session.
- The harness should give the agent normal project docs plus the architecture artifact, then ask it to perform a realistic follow-up task on the affected flow.
- Useful outputs include tokens consumed, files opened, wrong assumptions, invariants violated, success/failure, and whether the session regressed relative to base.
- This should remain a high-confidence evaluation mode, not a required check for every PR in v0.

---

## Success criteria

| Outcome area | What success looks like |
|:---|:---|
| Reviewer effectiveness | Reviewers find architectural mismatches and unsupported claims faster than with raw diff alone. |
| Direction clarity | Reviewers can explicitly say ‘direction wrong’, ‘implementation wrong’, or ‘cost unacceptable’ without collapsing these into one vague judgment. |
| Proof quality | High-risk claims arrive with better evidence or are blocked earlier. |
| Signed simplification credit | Cleanup and docs-sync changes are visibly rewarded with negative continuation deltas when deserved. |
| Cold-start quality | Future sessions—human or agent—need less context and make fewer wrong assumptions on flows improved by the system. |

---

## Open questions and unresolved edges

- Which architectural facts are worth extracting in v0, and which are too unreliable to include yet?
- How much compiler/build integration is necessary to make the diff trustworthy enough for real use?
- How do we prevent false precision in continuation-cost numbers and still keep the output actionable?
- Who owns the claim taxonomy and minimum evidence rules? Per org? Per language? Per repo?
- How do we avoid gaming, for example by adding shallow documentation or noisy abstractions just to move metrics?
- How should multi-repo and cross-service changes be represented when the architectural unit crosses repo boundaries?
- How do we treat generated code, vendor code, and migration-generated diffs?
- What privacy boundaries apply if prompts, traces, or runtime artifacts are attached to review objects?
- When should policy block a merge automatically versus requiring human sign-off?
- What is the minimal review packet that lets another agent continue the work safely without overfitting to the previous session’s story?

---

## Main risks and failure modes

| Risk | Why it matters | Mitigation posture |
|:---|:---|:---|
| False confidence | A polished architecture diff could make reviewers trust incomplete or inaccurate extraction too much. | Always show confidence/provenance and preserve raw drill-downs. |
| Over-ambitious v0 | Trying to support every language, every claim type, and every proof mode will stall the project. | Pick one language, a few hunk types, and a thin integration first. |
| Metric gaming | Teams may optimize for the reported score rather than for real maintainability. | Prefer signed deltas with named drivers over a single leaderboard number. |
| Reviewer friction | If artifact generation is noisy or slow, reviewers will ignore it. | Keep the default surface concise: a handful of semantic hunks, not a graph hairball. |
| Tool sprawl | The system could become an orchestration layer around too many analyzers with brittle maintenance. | Treat advanced analyzers as optional layers; keep a core path that is useful with simpler inputs. |

---

## Recommended first spike

A serious prototype should be small enough to finish and strong enough to falsify the idea if it does not work. The goal of the first spike is not to impress; it is to learn whether a semantic architecture review surface is materially better than raw diff for a subset of pull requests.

| Dimension | Recommendation | Why |
|:---|:---|:---|
| Language | Go or Java | Strong static information and realistic concurrency / API / service review scenarios. |
| Semantic hunk types | Call/control flow, lock/resource flow, state transition, public API delta | Enough to test the review model without boiling the ocean. |
| Integration | Standalone web review app + CI artifacts + optional PR annotations | Keeps product surface under control while allowing deep visuals. |
| Evidence sources | Tests, benchmark JSON, SARIF, basic traces, docs alignment checks | Enough to prove the claim-support model. |
| Evaluation set | 10 historical PRs + 3 seeded bug PRs | Allows side-by-side comparison against current review. |
| Exit decision | Proceed to full RFC only if reviewers prefer the architecture surface for at least some PR classes | No vanity prototype. |

> The exit question for the spike should be ruthless: do reviewers reach better architectural decisions faster on at least some pull-request classes? If not, the project should stop or narrow further instead of expanding scope.

---

## Appendix A. Research seed sources

The sources below are not the full literature review. They are the primary seeds that make the pre-RFC concrete and point to the areas that need deeper investigation before a full RFC.

| ID | Why it matters | Source |
|:---|:---|:---|
| R1 | Tree-sitter — incremental parsing library; useful for fast parsing and base/head structural extraction. | https://tree-sitter.github.io/tree-sitter/ |
| R2 | SCIP — language-agnostic source code indexing protocol used for precise code navigation. | https://github.com/sourcegraph/scip |
| R3 | LSIF — persisted language-server knowledge for code browsing without a live server. | https://lsif.dev/ |
| R4 | Glean at Meta — diff sketches and large-scale code facts; strong inspiration for machine-readable change summaries. | https://engineering.fb.com/2024/12/19/developer-tools/glean-open-source-code-indexing/ |
| R5 | Joern / Code Property Graph — unified syntax/control-flow/data-flow graph representation. | https://docs.joern.io/code-property-graph/ |
| R6 | SARIF — standard interchange format for static-analysis results. | https://docs.oasis-open.org/sarif/sarif/v2.1.0/sarif-v2.1.0.html |
| R7 | OpenTelemetry traces — traces as DAGs of spans; useful for runtime path evidence. | https://opentelemetry.io/docs/reference/specification/overview/ |
| R8 | Theia architecture — split frontend/backend processes and JSON-RPC boundary. | https://theia-ide.org/docs/architecture/ |
| R9 | Theia extension model — compile-time extensions with deep access and runtime plugin options. | https://theia-ide.org/docs/extensions/ |
| R10 | VS Code webviews — customizable UI surface with explicit caution about heavy resource cost. | https://code.visualstudio.com/api/extension-guides/webview |
| R11 | OPA / Rego — declarative policy-as-code for gating review decisions. | https://www.openpolicyagent.org/docs/policy-language |
| R12 | SLSA provenance — verifiable provenance model for how artifacts were produced. | https://slsa.dev/provenance |
| R13 | Apalache — symbolic model checker for TLA+; relevant for state/invariant checks. | https://apalache-mc.org/ |
| R14 | CBMC — bounded model checking for C/C++. | https://www.cprover.org/cbmc/ |
| R15 | KLEE — symbolic execution engine on LLVM bitcode. | https://klee-se.org/releases/docs/v2.3/ |
| R16 | CodeQL data flow and path queries — program-semantic and path-based analysis. | https://codeql.github.com/docs/writing-codeql-queries/about-data-flow-analysis/ |
| R17 | Semgrep taint analysis — source/sink/propagator model for dataflow checks. | https://semgrep.dev/docs/writing-rules/data-flow/taint-mode/overview |
| R18 | Dafny — verification-aware language and tooling. | https://dafny.org/ |
| R19 | Stack graphs — incremental name-resolution design reference; repository archived in September 2025. | https://github.com/github/stack-graphs |
| R20 | tree-sitter-graph — DSL for constructing graphs from parsed source code. | https://github.com/tree-sitter/tree-sitter-graph |

---

## Appendix B. What the full RFC should add

1. Narrowed problem statement tied to one target repository class and one target language.
2. Formal definitions for semantic hunk types and claim classes.
3. A versioned schema for the machine-readable review artifact.
4. Prototype screenshots and example PR walkthroughs.
5. Measured baseline data from real or replayed pull requests.
6. A specific decision on initial product surface: standalone app, IDE shell, or code-host-first integration.
7. Rollout, governance, provenance, privacy, and merge-policy design.
