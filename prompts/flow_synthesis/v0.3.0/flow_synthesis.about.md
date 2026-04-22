---
name: flow_synthesis.about
prompt_file: flow_synthesis.md
version: 0.3.0
curator: cairn-rs system-prompt-curator v2.0.0
type: reference
---

# About `flow_synthesis.md` v0.3.0

## Changes from v0.2.0

1. **Dropped the Explore phase.** The hunk list and the starter clusters are now injected by the host in the initial user message instead of requiring the model to call `adr.list_hunks` + `adr.list_flows_initial` as its first two turns. Qwen 3.5 27B on glide-mq #181 was spending 3–5 turns on exploration before starting synthesis; for large PRs that exceeded the practical turn budget. Eliminates two guaranteed round-trips.
2. **Renamed the reads namespace.** References to `adr.list_hunks` / `adr.list_flows_initial` are **absent** from the tool list section — they're not callable from this prompt's perspective. (The MCP server still registers them; v0.3.0 simply tells the model not to use them.)
3. **Hard "tools only, no prose" rule.** The v0.2.0 behaviour on Qwen 3.5 was to write the classification plan in markdown prose ("Proposed Flows: 1. …") instead of calling `adr.propose_flow`. The v0.3.0 "Hard Rules" section tells the model explicitly that prose after Phase 1 is discarded and must be replaced with tool calls.
4. **Parallel tool calls directive.** Qwen 3.5 batches by default; Gemma 4 does not. The prompt now encourages parallel `propose_flow` calls in a single assistant turn to reduce turn count.

## Placeholders (rendered at runtime)

- `{{hunk_count}}` — total hunks in the artifact.
- `{{initial_cluster_count}}` — structural clusters handed to the model.
- `{{max_tool_calls}}` — per-run tool call budget (default 200, capped by adr-mcp).

## Host responsibilities

Before the model sees this prompt, the host (adr-server) must:

1. Spawn `adr-mcp` and perform the MCP handshake.
2. Fetch `list_hunks` and `list_flows_initial` through the MCP child.
3. Embed the two JSON payloads in the first user message, e.g.:

   ```
   Hunks: [{...}, {...}]
   Initial clusters: [{...}, {...}]
   Synthesize the flows.
   ```

4. Send the system prompt (this file) + the composed user message as the first chat turn.

The model should reach `adr.finalize()` within 6–10 turns on a 12-hunk PR. On a 100-hunk PR, 15–25 turns.

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

- Gemma 4 26B stalls at 4 tool calls on glide-mq #181 (empty-response loop). Drop as primary.
- Qwen 3.5 27B reaches the planning phase but writes prose. v0.3.0 targets this specifically with the "Hard Rules" section.
- GLM (Zhipu) is the cloud fallback via paid API.
