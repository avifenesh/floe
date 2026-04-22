---
prompt_file: flow_synthesis.md
version: 0.2.0
status: draft
locks_at: scope 3 week 6
target_models:
  - gemma4:26b-a4b-it-q4_K_M
  - gemma4:e4b
  - qwen3.5:27b
applies_to: adr PI extension — flow-synthesis loop
wire_format: rendered once per analysis run, passed to PI via --system-prompt
curated_with: cairn-rs/docs/skills/system-prompt-curator (v2.0.0)
---

# About `flow_synthesis.md`

This file documents the prompt living next to it. The prompt file contains only the prompt; everything *about* the prompt is here.

## Purpose

System prompt given to the local LLM (Gemma 4 primary, Qwen 3.5 backup) when it runs inside PI via the `@adr/pi-extension`. Drives the flow-classification loop:

1. `adr:list_hunks()` + `adr:list_flows_initial()` to load context.
2. `adr:get_entity` / `adr:neighbors` / `read` to inspect uncertain hunks.
3. `adr:propose_flow` / `mutate_flow` / `remove_flow` to shape the flow set.
4. `adr:finalize()` as the gated exit.

## Template placeholders

Rendered by the Rust host at runtime. Do not commit the rendered output.

| Token | Type | Source |
|---|---|---|
| `{{hunk_count}}` | int | `artifact.hunks.length` |
| `{{initial_cluster_count}}` | int | `adr-flows` structural result |
| `{{max_tool_calls}}` | int | host config, default 200 |

## Design provenance

Written by applying the `system-prompt-curator` skill from cairn-rs (`docs/skills/system-prompt-curator/SKILL.md`, v2.0.0). Followed its 10 core principles:

1. Identity matches task — "senior software architect reviewing a pull request" rather than a generic agent label.
2. Autonomous completion — the prompt mandates working until every hunk has a flow and the host accepts.
3. Structured workflow phases — Explore → Investigate → Classify → Verify → Deliver. Each phase has a concrete exit condition.
4. Completion requires evidence — `adr:finalize()` is the gate; five completion criteria enumerated.
5. Tools listed upfront — all `adr:` and PI built-ins in one block; write/edit/bash explicitly marked unused.
6. Worked demonstration — full walkthrough on glide-mq PR #181 (real data from our analyzer).
7. Think-before-act transitions — Phase 2's inspection triage is the "think before you classify" beat.
8. Collaborative tone — no CRITICAL/MUST-FAILURE; uses "Do / Do not" lists.
9. Convention discovery before coding — "starting clusters are a draft, not a target" — treat structural output as existing convention to adapt, not replace.
10. Verification gate before completion — Phase 4 before Phase 5.

## Anti-patterns checked and cleared

From the skill's table:

- ✅ No "return complete_run immediately" pattern.
- ✅ No "if you can answer, complete" — completion requires every hunk covered + host accept.
- ✅ No CAPS emphasis; uses collaborative framing.
- ✅ Workflow phases present (5).
- ✅ Tools listed upfront, not discovered.
- ✅ One full worked trajectory with real data.
- ✅ Error-recovery section with actual host error codes.
- ✅ Completion requires concrete artifact (valid `adr:finalize()` accept).

## Changelog

- `0.2.0` (2026-04-18) — rewrote following cairn-rs `system-prompt-curator` template (identity-matched role, workflow phases, worked example, error recovery). Moved meta out of the prompt file.
- `0.1.0` (2026-04-18) — initial sketch, too brief, meta mixed with prompt body.

## Lock policy

Prompt locks at end of scope 3 week 6. After lock:

- Any prompt change forces re-running the eval set from scope 6.
- Bump `version` in this file and in the prompt's worked-example anchor.
- Keep a compatibility note: which model + PI version the new prompt was last verified against.

## Eval harness integration (scope 6)

The eval runner will render the prompt twice per PR — once for Gemma 4 26B MoE (primary), once for Gemma 4 E4B (floor). Both runs produce `artifact.flows[]` side by side. Reviewer A/B sees:

1. Raw-diff (GitHub) vs v0 surface with structural flows.
2. v0 surface with structural flows vs v0 surface with LLM-assisted flows.
3. LLM-assisted flows, 26B vs E4B.

Prompt must survive (3) — if the floor model can't produce valid flows on 50% of PRs with this prompt, the prompt is wrong or the task is too hard for the floor. The skill's "minimal" variant (~600 tokens) is the fallback for that case.

## Non-goals of the prompt

- Doesn't instruct on coding style, formatting, or file structure — the model doesn't write code.
- Doesn't describe flow-rendering semantics in the frontend. That's the host's concern.
- Doesn't specify MCP/socket protocol. That lives in `docs/adr-pi-extension.md`.
- Doesn't try to teach function-calling syntax. Gemma 4 and Qwen 3.5 both have it natively; PI handles the wrapper.
