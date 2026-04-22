# proof_verification prompt — metadata

**Version:** v0.1.0
**Curated with:** cairn-rs `system-prompt-curator` skill v2.0.0
**Sibling prompt file:** `proof_verification.md`

## Purpose

Per-flow proof-verification verdict: is there real evidence for the PR's stated claims? Bench numbers in notes, `examples/*` files exercising the claimed behaviour, tests that assert the specific claim. Output is a single JSON object matching [`adr_core::intent::Proof`](../../../crates/adr-core/src/intent.rs).

This is the pass that determines the Proof section of the product. Per Avi (`feedback_proof_not_tests.md`), unit-test presence is explicitly *not* proof — only claim-asserting tests, examples, benchmarks, or corroborating observations count.

## Placeholders

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{max_tool_calls}}` | Config-passed | Default 15. Higher than intent-fit because evidence-hunting requires grep + read round-trips. |

No other placeholders. Intent / flow / notes render into the *user* message.

## Lock policy

Same policy as `intent_fit.about.md`. Version directory bumps on any change.

## Anti-pattern check (per cairn-rs curator skill)

- ✅ **Negative rule surfaced loudly.** The identity section leads with "a unit test that … does not assert the specific claim is NOT proof" — this is the rule most reviewers (and models) get wrong, so it gets top billing.
- ✅ **Worked example marked INVENTED.** The back-pressure/stream example is synthetic.
- ✅ **Output format pinned.** Exactly one JSON object as final message, schema stated inline.
- ✅ **Budget named.** 15 tool calls, stated.
- ✅ **Hunt order specified.** Phase 1 enumerates where to look (notes → examples → claim-asserting tests → benches), in that order — reduces the Gemma/GLM failure mode of grep-spraying early and burning budget.
- ✅ **No sycophancy.** Declarative throughout.

## Strength capping

If a reviewer runs this pass on a local small model instead of GLM-4.7 (against the `feedback_proof_uses_glm.md` rule), the pipeline MUST cap any emitted `strength` at `medium` and stamp the model name into the provenance. That's a pipeline concern, not a prompt concern — but documented here so the two stay in sync.

## Calibration notes

Expected signal at v0.1.0:

- **Structured intent with bench claim + notes bench:** verdict `strong` with `claim_index=0` filled.
- **Structured intent with example claim, no `examples/` dir:** verdict `missing`.
- **Raw-text intent with no benchmark, no examples:** verdict `missing` with `claim_index: -1`.
- **Empty/no-intent:** verdict `no-intent` (hard-coded short-circuit in the pipeline before the LLM runs, but the prompt still handles the case as a fallback).

Calibration against glide-mq PR #181 pending — the PR doesn't have structured intent, so the first live test will be with a hand-authored intent.json.
