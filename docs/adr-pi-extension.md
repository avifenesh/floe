# `adr` PI extension — tool contract  *(HISTORICAL / SUPERSEDED)*

> **Status: superseded 2026-04-22.**
> PI (Ollama's minimal coding agent) was dropped after its per-run
> extension API turned out to be undocumented in pi-mono. Replaced by
> `adr-mcp` — a Rust binary speaking MCP-over-stdio JSON-RPC 2.0.
> `adr-server` spawns it as a child per analysis; the tool surface,
> error codes, and host invariants below survived the pivot intact.
>
> **Current source of truth** for the tool contract is the code —
> `crates/adr-mcp/src/handlers.rs`, `wire.rs`, `errors.rs`, plus the
> `session_scripts.rs` integration tests. See also RFC v0.3 §5.
>
> This file is preserved as design history; do not rely on its socket /
> launch / pi-extension details.

---

Status: **draft v0.1** · 2026-04-18 · locks at end of scope 3 week 6

The `@adr/pi-extension` extension runs inside PI (Ollama's minimal coding agent). PI already gives the model `read` / `grep` / `glob` for inspecting source. The extension adds:

- **read tools** that expose the analyzed artifact (the graph, hunks, starting clusters) — stable `adr:list_hunks`, `adr:get_entity`, `adr:neighbors`, `adr:list_flows_initial`
- **mutation tools** that let the model record flow assignments, validated host-side — `adr:propose_flow`, `adr:mutate_flow`, `adr:remove_flow`, `adr:finalize`

The host (Rust, in `adr-mcp`) exposes the artifact over a per-run local socket (Unix domain socket on POSIX, named pipe on Windows) and validates every mutation before accepting it. Whole-run rejection on any invariant violation.

---

## Wire protocol

Per analysis run:

1. `adr-server` writes the artifact to a temp path.
2. `adr-mcp` binds a per-run socket, e.g. `\\.\pipe\adr-<jobId>` on Windows, `/tmp/adr-<jobId>.sock` on POSIX.
3. Backend invokes: `ollama launch pi --model <model> -- --extension @adr/pi-extension --adr-socket <path> --adr-artifact <path>`.
4. PI starts, loads the extension; the extension connects to the socket and reads the artifact metadata.
5. PI's model is seeded with the initial prompt (below), sees the tools from the extension registered in its tool schema, and runs its loop.
6. On `adr:finalize`, the extension posts final flow list to the host socket and waits for accept/reject.
7. Host runs invariants, accepts → writes `artifact.flows[]` with `source: "llm:<model>@<version>"`. Rejects → frontend sees banner + structural fallback.

All JSON over length-prefixed newline-delimited records. No HTTP; no cross-origin concerns.

---

## Initial prompt (frozen at scope 3 week 6)

Lives in `crates/adr-mcp/prompts/flow_synthesis.md` and is versioned. Single source of truth, templated at runtime with the hunk count and structural cluster count.

Key instructions to the model:

- You are classifying hunks into flows — not writing code.
- Use `adr:list_hunks` and `adr:list_flows_initial` to see the starting point.
- Call `adr:get_entity` / `adr:neighbors` / PI's `read` / PI's `grep` to inspect what you don't understand.
- Every hunk must end up in at least one flow. A hunk may be in multiple flows — do that when two flows both architecturally touch it.
- Every entity you reference must exist — use the IDs from `adr:get_entity`. Never invent.
- Give each flow a short concrete name derived from *what it does*, not *what it lives in*. "Multi-metric budget support" beats "Queue methods".
- When done, call `adr:finalize`.

---

## Read tools

### `adr:list_hunks()`

Returns an array of hunk summaries in stable order:

```jsonc
[
  {
    "id": "hunk-e3...",           // stable across runs for same artifact
    "kind": "call" | "state" | "api",
    "summary": "Queue.setBudget signature widened to multi-metric",
    "entities": ["node-1af2", "node-c3d8"],  // node IDs affected by this hunk
    "side": "added" | "removed" | "both"
  },
  ...
]
```

### `adr:get_entity(id: string)`

Returns a node descriptor:

```jsonc
{
  "id": "node-1af2",
  "kind": "function" | "type" | "state" | "api-endpoint" | "file",
  "name": "Queue.setBudget",      // qualified name (ClassName.methodName for methods)
  "file": "src/queue.ts",
  "span": { "start_line": 124, "end_line": 142 },
  "side": "head" | "base",
  "signature"?: "async setBudget(flowId: string, budget: { ... }): void"  // functions/methods only
}
```

### `adr:neighbors(id: string, hops?: number = 1)`

Returns a subgraph centred on `id`:

```jsonc
{
  "nodes": [...],  // entity descriptors, same shape as get_entity
  "edges": [
    { "from": "node-1af2", "to": "node-c3d8", "kind": "calls" | "defines" | "exports" | "transitions" }
  ]
}
```

`hops` capped at 3 by the host. Use sparingly on large graphs.

### `adr:list_flows_initial()`

Returns the deterministic structural clustering that already ran:

```jsonc
[
  {
    "id": "structural-0",
    "name": "<structural: Queue cluster>",
    "rationale": "call-graph connected component + shared parameter types",
    "hunk_ids": ["hunk-e3...", "hunk-a7..."],
    "entities": ["node-1af2", ...],
    "confidence": "structural"
  },
  ...
]
```

Starting point, not the answer. The model is expected to merge / split / rename / re-entities.

---

## Mutation tools (host-validated)

### `adr:propose_flow(name, rationale, hunk_ids, extra_entities?)`

Accepts a new flow candidate.

- `name`: string, 3..48 chars, not in the reserved list (`{"misc", "various", "other", "unknown", "cluster", "group"}` — those are reserved for the fallback bucket).
- `rationale`: string, 1..240 chars.
- `hunk_ids`: string[], each must appear in `adr:list_hunks()`.
- `extra_entities`?: string[], each must appear via `adr:get_entity()`. These are non-hunk entities the LLM is adding to express the flow's reach (unchanged callers/callees that belong to the flow).

Returns `{ flow_id }` on accept, or `{ error, reason }` on reject.

### `adr:mutate_flow(flow_id, patch)`

Applies a partial patch:

```jsonc
{
  "name"?: "new name",
  "rationale"?: "new rationale",
  "add_hunks"?: [...],
  "remove_hunks"?: [...],
  "add_entities"?: [...],
  "remove_entities"?: [...]
}
```

Host re-runs per-flow invariants after the patch.

### `adr:remove_flow(flow_id)`

Drops the flow entirely. Host pre-checks that every one of that flow's hunks is still covered by another flow after removal — otherwise rejected.

### `adr:finalize()`

Model signals it's done.

Host runs global invariants:

1. Every hunk from `list_hunks` appears in ≥ 1 flow.
2. No flow has `name` in the reserved list (except the auto-created `misc` fallback, which the LLM cannot produce but the host may synthesize post-hoc if needed — though v0 policy is to reject rather than silently backfill).
3. No flow references an entity or hunk that doesn't exist.
4. Tool-call budget not exceeded (cap: 200 calls per run).

If all pass: persists `artifact.flows[]` with `source: "llm:<model>@<runtime-version>"`. Returns `{ accepted: true, flows }`.

If any fail: `{ accepted: false, reason, rejected_rule }`. Backend logs, user sees structural banner.

---

## Error model

Each mutation tool returns:

```jsonc
{ "ok": true, "result": ... }
// or
{ "ok": false, "error": "<code>", "reason": "<human-readable>" }
```

Error codes (non-exhaustive, frozen at week 6):

- `NAME_RESERVED` — `name` in reserved list
- `NAME_TOO_SHORT` / `NAME_TOO_LONG`
- `HUNK_NOT_FOUND`
- `ENTITY_NOT_FOUND`
- `FLOW_NOT_FOUND`
- `COVERAGE_BROKEN` — removing this flow would orphan a hunk
- `CALL_BUDGET_EXCEEDED`

The LLM is expected to retry with corrections; we do not penalise a single retry.

---

## Context budget (per-model mode)

| Model | Mode | Strategy |
|---|---|---|
| Gemma 4 26B MoE (256 K ctx) | single-pass | Everything in one prompt: `list_hunks` + `list_flows_initial` upfront in the system message; model does all mutations in one session. |
| Qwen 3.5 27B dense (256 K ctx) | single-pass | Same as above. |
| Gemma 4 E4B (128 K ctx) | flow-by-flow | For each structural cluster: seed a fresh PI session with only that cluster + neighborhood; model keeps/splits/merges/renames. A final pass reads all accepted flows and does cross-flow moves only. |

Mode is picked by the host based on model name + artifact size. Override via `.adr/llm.toml::mode`.

---

## Non-goals of this extension

- The extension does **not** write flows directly to the artifact. Only the Rust host does, after validation.
- The extension does **not** call Ollama. PI does.
- The extension does **not** run without a host socket — no standalone mode.
- The LLM is **not** trusted to produce running code. PI's `write` / `edit` / `bash` outputs are ignored by our artifact layer; whatever the model writes to disk during its turn is not accepted into the artifact.

---

## Versioning

- Extension package: `@adr/pi-extension` — semver. Major = wire-breaking.
- Host-side host: `adr-mcp` crate — versioned alongside the rest of the workspace.
- Compatibility matrix maintained in `crates/adr-mcp/COMPAT.md` when we actually break the wire.
- For v0: treat the wire as **pinned**. Any change during scopes 5–6 requires re-running the eval.
