---
name: flow_synthesis.about
prompt_file: flow_synthesis.md
version: 0.3.1
curator: cairn-rs system-prompt-curator v2.0.0
type: reference
---

# About `flow_synthesis.md` v0.3.1

## Changes from v0.3.0

- **Stronger "small flows are fine" directive.** v0.3.0's single bullet was under-weighted — on glide-mq #181, Qwen 3.5 27B folded `Job.consumeSuspendRequest` (a nullability tweak) into the "Streaming chunk API" flow because both touched `Job`. v0.3.1 promotes this to a full rule with concrete examples (nullability tweak, signature widening, formatting change, side cleanup) and states explicitly that over-grouping is worse than over-splitting.
- **New rule: "intent, not class."** Two methods on the same class can belong to different flows. The prompt now says this out loud.
- **Worked example annotated.** The single-hunk "Structured error logging" flow carries an inline comment explaining why it stands alone.

## Changes from v0.2.0 (carry-forward)

1. **Dropped the Explore phase.** The hunk list and the starter clusters are now injected by the host in the initial user message instead of requiring the model to call `floe.list_hunks` + `floe.list_flows_initial` as its first two turns. Qwen 3.5 27B on glide-mq #181 was spending 3–5 turns on exploration before starting synthesis; for large PRs that exceeded the practical turn budget. Eliminates two guaranteed round-trips.
2. **Renamed the reads namespace.** References to `floe.list_hunks` / `floe.list_flows_initial` are **absent** from the tool list section — they're not callable from this prompt's perspective. (The MCP server still registers them; v0.3.0 simply tells the model not to use them.)
3. **Hard "tools only, no prose" rule.** The v0.2.0 behaviour on Qwen 3.5 was to write the classification plan in markdown prose ("Proposed Flows: 1. …") instead of calling `floe.propose_flow`. The v0.3.0 "Hard Rules" section tells the model explicitly that prose after Phase 1 is discarded and must be replaced with tool calls.
4. **Parallel tool calls directive.** Qwen 3.5 batches by default; Gemma 4 does not. The prompt now encourages parallel `propose_flow` calls in a single assistant turn to reduce turn count.

## Placeholders (rendered at runtime)

- `{{hunk_count}}` — total hunks in the artifact.
- `{{initial_cluster_count}}` — structural clusters handed to the model.
- `{{max_tool_calls}}` — per-run tool call budget (default 200, capped by floe-mcp).

## Host responsibilities

Before the model sees this prompt, the host (floe-server) must:

1. Spawn `floe-mcp` and perform the MCP handshake.
2. Fetch `list_hunks` and `list_flows_initial` through the MCP child.
3. Embed the two JSON payloads in the first user message, e.g.:

   ```
   Hunks: [{...}, {...}]
   Initial clusters: [{...}, {...}]
   Synthesize the flows.
   ```

4. Send the system prompt (this file) + the composed user message as the first chat turn.

The model should reach `floe.finalize()` within 6–10 turns on a 12-hunk PR. On a 100-hunk PR, 15–25 turns.

## Anti-patterns checklist (cairn-rs skill)

- ✓ Identity matches task (architect, not generic agent)
- ✓ Structured workflow phases
- ✓ Completion gate before finalize
- ✓ Tools listed upfront, not discovered
- ✓ Worked example (hypothetical — not the glide-mq test fixture)
- ✓ Think-before-act for investigation only
- ✓ Collaborative tone (no CAPS emphasis except when asked to stop doing something)
- ✓ Error recovery section

## Eval notes

- **glide-mq #181 baseline (v0.3.0 + Qwen 3.5 27B Q4_K_M + pre-inject):** 2 flows accepted in 5 turns, 3m10s wall time. Names: "Streaming chunk API", "Multi-metric budget tracking". Both rationales reference concrete architectural signals. Failure: `Job.consumeSuspendRequest` absorbed into the streaming flow — v0.3.1 targets this.
- **glide-mq #181 baseline (v0.3.0 + Gemma 4 26B):** Fails. Model stalls after 4 exploration tool calls without proposing any flows. Dropped as primary.
- GLM (Zhipu) is the cloud fallback via paid API, planned for scope 6 calibration.
