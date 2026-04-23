# intent_fit prompt — metadata

**Version:** v0.1.0
**Curated with:** cairn-rs `system-prompt-curator` skill v2.0.0
**Sibling prompt file:** `intent_fit.md`

## Purpose

Per-flow intent-fit verdict: does this flow deliver a claim the PR's stated intent makes? Input is the flow + the structured (or raw-text) intent + reviewer notes. Output is a single JSON object matching [`floe_core::intent::IntentFit`](../../../crates/floe-core/src/intent.rs).

## Placeholders

The prompt body expands exactly one placeholder:

| Placeholder | Source | Notes |
|-------------|--------|-------|
| `{{max_tool_calls}}` | Config-passed | Default 10. Cap on `adr.*` reads per run. |

No other placeholders. Intent / flow / notes are rendered by the pipeline into the *user* message, not into the system prompt — so prompt stays stable across PRs and the cache key only invalidates on explicit prompt-version bumps.

## Lock policy

Changing this file bumps the directory version (`v0.1.0` → `v0.1.1` for wording tweaks, `v0.2.0` for shape changes, `v1.0.0` once calibration lands). Breaking the output JSON shape is a minor bump; shape changes at v1 require a semver-major.

## Anti-pattern check (per cairn-rs curator skill)

- ✅ **No sycophancy.** The rules sections are declarative, not encouraging.
- ✅ **Worked example marked INVENTED.** The Redis/Queue example is synthetic — no risk of the model regurgitating the example into real output. See `feedback_prompt_hygiene.md`.
- ✅ **Output format pinned.** "Exactly one JSON object as your final message" — not "please output JSON" or "use a tool call if you want".
- ✅ **Budget named.** `{{max_tool_calls}}` is stated so the model self-paces.
- ✅ **Negative examples captured in rules.** "Do not use tools to read intent or flow data — those are already in your context" heads off the observed Gemma habit of re-fetching context.

## Calibration notes

Calibration pass deferred to scope 6. Expected signal: intent-fit verdicts should be stable across reruns on the same (artifact, intent) pair; `strength=high` should match a reviewer reading the same hunk list 80%+ of the time.

## Model expectations

- Primary: GLM-4.7 (cloud, `FLOE_PROOF_LLM=glm:glm-4.7` or default). Per `feedback_proof_uses_glm.md`, intent-fit reads prose intent + semantic code; small local models hallucinate. If ever run locally, cap emitted `strength` at Medium.
