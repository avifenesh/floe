//! Flow-membership probe — experimental.
//!
//! Single GLM-4.7 session per flow that picks which surrounding
//! entities actually participate in the architectural story and
//! describes the call shape between them. Not wired into the
//! pipeline yet — called from the `floe-flow-membership` binary so
//! we can observe raw model output on a real artifact before
//! committing to a JSON contract or a parser.
//!
//! # Logging
//!
//! Every prompt, tool call, tool response (as byte count), and the
//! model's final content string lands in `tracing::info!` with
//! target `flow_membership`. Run with
//! `RUST_LOG=flow_membership=info,floe_server=info` to see the
//! full round-trip.
//!
//! # Output
//!
//! Returns the model's final message content *verbatim*. No parsing,
//! no shape validation. The point of this scaffolding is to see what
//! GLM-4.7 actually emits; hardening comes after.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use serde_json::json;

use super::config::LlmConfig;
use super::glm_client::GlmClient;
use super::mcp_client::{McpClient, ToolSpec};
use super::ollama_client::{ChatMessage, ChatRequest, ToolDef, ToolDefFunction};
use floe_core::{Artifact, Flow, FlowMembership};

/// Parse the model's final message. Strips an optional ```json fence
/// (some GLM responses wrap even when told not to) and defers to
/// `serde_json::from_str`. Returns the raw tuple for debugging:
/// `(parsed, leftover_text_if_any)`.
pub fn parse_response(raw: &str) -> Result<FlowMembership> {
    let trimmed = raw.trim();
    let body = strip_code_fence(trimmed);
    let parsed: FlowMembership = serde_json::from_str(body)
        .with_context(|| format!("parsing membership JSON (head: {})", head(body, 120)))?;
    Ok(parsed)
}

fn strip_code_fence(s: &str) -> &str {
    let s = s.trim();
    if let Some(rest) = s.strip_prefix("```json") {
        return rest.trim().trim_end_matches("```").trim();
    }
    if let Some(rest) = s.strip_prefix("```") {
        return rest.trim().trim_end_matches("```").trim();
    }
    s
}

fn head(s: &str, n: usize) -> String {
    s.chars().take(n).collect::<String>()
}

/// Drop noise the model occasionally emits: file-path pseudo-entities
/// (`src/queue.ts`), blank names, excessive cap violations. Leaves
/// valid content untouched. Idempotent.
pub fn sanitize(mut m: FlowMembership) -> FlowMembership {
    let is_file_path = |s: &str| {
        s.contains('/')
            || s.ends_with(".ts")
            || s.ends_with(".tsx")
            || s.ends_with(".js")
            || s.ends_with(".jsx")
            || s.ends_with(".rs")
    };
    m.members.retain(|mem| !mem.entity.trim().is_empty() && !is_file_path(&mem.entity));
    m.members.truncate(10);
    for g in m.summary_groups.iter_mut() {
        g.sample_entities
            .retain(|e| !e.trim().is_empty() && !is_file_path(e));
    }
    m.summary_groups
        .retain(|g| !g.label.trim().is_empty());
    m.summary_groups.truncate(10);
    m.edges.retain(|e| {
        !e.from.trim().is_empty()
            && !e.to.trim().is_empty()
            && !is_file_path(&e.from)
            && !is_file_path(&e.to)
    });
    m.shapes.retain(|s| !s.kind.trim().is_empty());
    m
}

/// Max GLM turns. Each turn = one model response (possibly with tool
/// calls). 10 covers mid-sized flows comfortably while still stopping
/// runaway exploration; the turn-remaining nudge fires at turn 8 and
/// tools are stripped on turn 9 so the model has a clean "emit JSON"
/// turn left even when it paced itself badly.
const MAX_TURNS: u32 = 10;

/// Per-session input-token cap. Big `list_entities` responses can
/// inject ~170KB of JSON into context in one turn; every subsequent
/// turn re-sends that payload. Abort when we cross this line so a
/// single flow can't burn through the user's GLM budget silently.
const MAX_SESSION_TOKENS_IN: u32 = 40_000;

/// Tools exposed to the membership model. Read-only subset — the
/// probe never mutates the artifact.
const MEMBERSHIP_TOOLS: &[&str] = &[
    "floe.get_entity",
    "floe.neighbors",
    "floe.list_entities",
    "floe.read_file",
    "floe.grep",
    "floe.glob",
];

/// Run one membership session for the given flow. Returns the raw
/// text of the model's final content message. Caller prints / logs
/// it; no downstream coupling yet.
pub async fn probe(
    artifact: &Artifact,
    flow_id: &str,
    llm_cfg: &LlmConfig,
    head_root: &Path,
    artifact_path: &Path,
    progress: Option<&crate::job::TurnProgressMap>,
) -> Result<String> {
    let flow = artifact
        .flows
        .iter()
        .find(|f| f.id == flow_id)
        .ok_or_else(|| anyhow!("flow {flow_id} not in artifact"))?;

    let system_prompt = build_system_prompt();
    let user_prompt = build_user_prompt(artifact, flow);

    tracing::info!(
        target: "flow_membership",
        flow_id = %flow.id,
        flow_name = %flow.name,
        seed_entities = flow.entities.len(),
        model = %llm_cfg.model,
        "starting membership probe"
    );
    tracing::info!(target: "flow_membership", system_prompt_bytes = system_prompt.len());
    tracing::info!(target: "flow_membership", user_prompt_bytes = user_prompt.len(), "user prompt:\n{user_prompt}");

    let mut mcp = McpClient::spawn_proof(
        artifact_path,
        &llm_cfg.model,
        "flow-membership-probe",
        head_root,
    )
    .await
    .context("spawn floe-mcp (proof mode)")?;
    mcp.initialize().await.context("mcp initialize")?;
    let tool_specs = mcp.list_tools().await.context("tools/list")?;
    let tools = build_tool_defs(&tool_specs);
    tracing::info!(
        target: "flow_membership",
        mcp_tool_count = tool_specs.len(),
        exposed_tool_count = tools.len(),
        "mcp ready"
    );

    let base_url = if llm_cfg.base_url.is_empty() {
        super::glm_client::default_base_url().to_string()
    } else {
        llm_cfg.base_url.clone()
    };
    let api_key = llm_cfg
        .api_key
        .clone()
        .ok_or_else(|| anyhow!("GLM api key missing (set FLOE_GLM_API_KEY)"))?;
    let client = GlmClient::new(base_url, api_key);

    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage {
            role: "system".into(),
            content: system_prompt,
            tool_calls: Vec::new(),
            tool_name: None,
        },
        ChatMessage {
            role: "user".into(),
            content: user_prompt,
            tool_calls: Vec::new(),
            tool_name: None,
        },
    ];

    let mut final_content: Option<String> = None;
    // Turn-budget landmarks. Telling the model "you have N turns
    // left" on the second-to-last turn and stripping tools on the
    // last turn forces it to emit JSON instead of keep calling
    // tools until it runs out and returns empty content.
    let warn_turn = MAX_TURNS.saturating_sub(2); // turn 4 of 6
    let final_turn = MAX_TURNS.saturating_sub(1); // turn 5 of 6
    let mut warned = false;
    for turn in 0..MAX_TURNS {
        tracing::info!(target: "flow_membership", turn, "chat");
        if let Some(p) = progress {
            // `membership:<flow_id>` so the UI can show one bar per
            // flow. Current is turn + 1 so it advances to 1 on the
            // first iteration (reader expects 1-indexed).
            p.mark(&format!("membership:{flow_id}"), turn + 1, MAX_TURNS);
        }
        if turn == warn_turn && !warned {
            messages.push(ChatMessage {
                role: "user".into(),
                content: format!(
                    "Turn {turn} of {MAX_TURNS}. You have {} turns left. \
                     Commit now: emit the JSON on your next turn, even \
                     if you haven't verified every member. An honest \
                     partial answer beats no answer.",
                    MAX_TURNS - turn
                ),
                tool_calls: Vec::new(),
                tool_name: None,
            });
            warned = true;
        }
        let tools_for_turn = if turn >= final_turn {
            // Strip tools on the last turn so the model physically
            // cannot defer emission by calling another tool.
            messages.push(ChatMessage {
                role: "user".into(),
                content: "LAST TURN. No more tool calls. Emit the JSON \
                         object now — one object, no prose, no fence."
                    .into(),
                tool_calls: Vec::new(),
                tool_name: None,
            });
            Vec::new()
        } else {
            tools.clone()
        };
        let req = ChatRequest {
            model: llm_cfg.model.clone(),
            messages: messages.clone(),
            tools: tools_for_turn,
            stream: false,
            options: None,
            keep_alive: Some(llm_cfg.keep_alive.clone()),
        };
        let resp = client.chat(req).await.context("glm chat")?;
        let msg = resp.message.clone();
        tracing::info!(
            target: "flow_membership",
            turn,
            tokens_in = resp.tokens_in,
            tokens_out = resp.tokens_out,
            content_bytes = msg.content.len(),
            tool_calls = msg.tool_calls.len(),
            "turn result"
        );
        if resp.tokens_in > MAX_SESSION_TOKENS_IN {
            tracing::warn!(
                target: "flow_membership",
                turn,
                tokens_in = resp.tokens_in,
                cap = MAX_SESSION_TOKENS_IN,
                "aborting session — token budget exceeded (list_entities / read_file response too big)"
            );
            let _ = mcp.shutdown().await;
            return Err(anyhow!(
                "membership session exceeded {MAX_SESSION_TOKENS_IN} input tokens in one turn (got {}); stop to avoid runaway GLM cost",
                resp.tokens_in
            ));
        }
        messages.push(msg.clone());

        if !msg.tool_calls.is_empty() {
            for call in &msg.tool_calls {
                let name = call.function.name.clone();
                let args_str = serde_json::to_string(&call.function.arguments)
                    .unwrap_or_else(|_| "<unserializable>".into());
                tracing::info!(
                    target: "flow_membership",
                    turn,
                    tool = %name,
                    args_bytes = args_str.len(),
                    "tool call: {args_str}"
                );
                if !MEMBERSHIP_TOOLS.contains(&name.as_str()) {
                    let err = json!({
                        "error": format!("tool `{name}` not in allowlist")
                    });
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: err.to_string(),
                        tool_calls: Vec::new(),
                        tool_name: Some(name),
                    });
                    continue;
                }
                let result = match mcp
                    .call_tool(&name, call.function.arguments.clone())
                    .await
                {
                    Ok(r) => r,
                    Err(e) => {
                        tracing::warn!(
                            target: "flow_membership",
                            tool = %name,
                            error = %e,
                            "mcp call errored"
                        );
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: json!({"error": e.to_string()}).to_string(),
                            tool_calls: Vec::new(),
                            tool_name: Some(name),
                        });
                        continue;
                    }
                };
                let result_json = if result.is_error {
                    json!({"error": result.text}).to_string()
                } else {
                    result.text.clone()
                };
                tracing::info!(
                    target: "flow_membership",
                    turn,
                    tool = %name,
                    response_bytes = result_json.len(),
                    "tool response"
                );
                messages.push(ChatMessage {
                    role: "tool".into(),
                    content: result_json,
                    tool_calls: Vec::new(),
                    tool_name: Some(name),
                });
            }
            continue;
        }

        // No tool calls → this is the final answer.
        if !msg.content.is_empty() {
            tracing::info!(
                target: "flow_membership",
                turn,
                final_content_bytes = msg.content.len(),
                "final content:\n{}",
                msg.content
            );
            final_content = Some(msg.content);
            break;
        }

        tracing::warn!(target: "flow_membership", turn, "empty turn — neither tool calls nor content");
    }

    let _ = mcp.shutdown().await;

    final_content.ok_or_else(|| anyhow!("session ended without final content"))
}

fn build_system_prompt() -> String {
    r#"You are a senior code reviewer curating ONE flow of a TypeScript PR.
You have 10 turns total to investigate and commit an answer. Your output
is a single JSON object; committing it ends the task. Running out of
turns without emitting JSON is a failed task.

# What a flow is

The smallest group of entities that carry ONE architectural story
end-to-end (entrance → middle → end), not just the literal delta.

# Output contract — this is the task

On your LAST turn (no later than turn 6), emit ONE JSON object, no
prose, no code fence, no explanation:

{
  "members": [
    {
      "entity": "<qualified name — e.g. Queue.enqueue, sendWithRetry, JobPayload>",
      "role": "core" | "entrance" | "exit",
      "side": "<free-form label, e.g. 'queue', 'client', 'transport'>",
      "why": "<one short sentence>"
    }
  ],
  "summary_groups": [
    {
      "label": "<e.g. test scaffolding>",
      "count": <int>,
      "sample_entities": ["<qualified name>"],
      "note": "<one sentence>"
    }
  ],
  "edges": [
    {
      "from": "<qualified name>",
      "to": "<qualified name>",
      "kind": "call" | "data-flow" | "transition",
      "note": "<optional>"
    }
  ],
  "shapes": [
    { "kind": "loop",   "nodes": ["A", "B"] },
    { "kind": "branch", "at": "A", "paths": [["B"], ["C"]] },
    { "kind": "fanout", "from": "A", "to": ["B", "C", "D"] }
  ],
  "diagrams": [
    {
      "kind": "mermaid",
      "label": "head",
      "source": "<Mermaid spec for the head (current) flow — see below>"
    },
    {
      "kind": "mermaid",
      "label": "base",
      "source": "<Mermaid spec for the base (previous) flow, ONLY when the flow's shape actually differs from head>"
    }
  ]
}

# Mermaid diagrams

Emit ONE diagram for `head` always. Emit a second `base` diagram ONLY
when the flow's shape — who calls whom, the branches, the loops —
genuinely differs between base and head. If base is "same shape minus
a couple of added entities", don't emit it; the head diagram's
additions/removals classes already tell that story.

Pick the notation per flow:

- `flowchart TD` for call chains with branches and loops (most common).
- `stateDiagram-v2` for state machines (transitions between discrete states).
- `sequenceDiagram` for ordered cross-entity call sequences.

## Rules for every diagram

- Node ids / participants are the qualified entity names from
  `members`. Quote when the name contains dots:
  `A["Queue.enqueue"]`.
- Loops: draw the back-edge explicitly (`A --> B; B --> A`).
- Branches / ifs: annotate with `-->|label|` (e.g. `-->|success|`).
- Keep each diagram under ~25 lines.

## Marking the diff on the head diagram

Head should visibly distinguish what was added, removed, or
unchanged vs base. Use three reserved class names — the UI styles
them consistently across diagrams:

- `:::added`      — node exists only in head.
- `:::removed`    — node exists only in base (include as a ghost
  node in the head diagram so the diff reads inline).
- `:::unchanged`  — default; omit if you want (no class = unchanged).

Include the classDef block exactly as below so the UI can theme:

```
classDef added    fill:#d1fae5,stroke:#059669,color:#065f46
classDef removed  fill:#fee2e2,stroke:#dc2626,color:#991b1b,stroke-dasharray:4 3
classDef unchanged fill:#f4f4f5,stroke:#a1a1aa,color:#3f3f46
```

## Example — head flowchart with a retry path added

```
flowchart TD
  A["Queue.enqueue"]:::unchanged --> B["sendWithRetry"]:::added
  B --> C["send"]:::unchanged
  C -->|success| Done([return]):::unchanged
  C -->|fail| B
  classDef added    fill:#d1fae5,stroke:#059669,color:#065f46
  classDef removed  fill:#fee2e2,stroke:#dc2626,color:#991b1b,stroke-dasharray:4 3
  classDef unchanged fill:#f4f4f5,stroke:#a1a1aa,color:#3f3f46
```

## Example — base diagram (when shape differs)

```
flowchart TD
  A["Queue.enqueue"] --> C["send"]
  C --> Done([return])
```

Bounds: `members` ≤ 10; `summary_groups` ≤ 10; entity names must be
real qualified names (never invent, never use file paths like
`src/queue.ts`). If the flow is self-contained, members = seed
entities only, summary_groups = [], shapes describe the internal
pattern.

# Workflow — 10 turns, phase by phase

- Turn 1-3 — initial investigation. One tool call per turn. Usually
  `floe.get_entity` then `floe.neighbors(id, hops=1)` on seed
  entities. By turn 3 you should know who calls the flow and who
  it calls.
- Turn 4-7 — targeted deep dive. `floe.read_file` with offset+limit
  or `floe.grep` with a tight pattern when a specific method's
  behaviour is unclear. Skip turns you don't need.
- Turn 8 — pre-commit check. Only one more tool call if it truly
  changes your answer.
- Turn 9 — commit. Emit the JSON. No tool calls; tools will be
  stripped automatically.

You may call multiple tools in one turn, but each tool call burns
the same per-turn context — prefer one per turn unless you're
fetching truly independent entities.

# Tools

- `floe.get_entity(id)` — node descriptor (name, file, signature).
- `floe.neighbors(id, hops=1)` — 1-hop callers/callees. Prefer
  hops=1; hops=2 can return large responses.
- `floe.read_file(path, offset, limit)` — source. ALWAYS pass
  offset+limit on files larger than ~100 lines.
- `floe.grep(pattern, path)` — tight pattern search. Preferred over
  read_file when you're looking for specific definitions.
- `floe.glob(pattern)` — file listing. Rarely useful here.
- `floe.list_entities` — banned for this task; the seed names are
  in the user prompt, and its response can blow the context budget.

# Commit discipline

- The task is NOT to investigate everything; it's to commit a
  best-faith JSON within 10 turns.
- An incomplete but honest answer (e.g. only seeds as members, no
  shapes, empty summary_groups) is better than no answer.
- If a tool call fails or returns an error, try ONCE more with
  different parameters, then commit what you have.
- Do not re-read the same file or repeat a `neighbors` call.

When in doubt, commit.
"#
    .to_string()
}

fn build_user_prompt(artifact: &Artifact, flow: &Flow) -> String {
    let mut out = String::new();
    out.push_str(&format!(
        "Flow id: {}\nFlow name: {}\nRationale: {}\n\n",
        flow.id, flow.name, flow.rationale
    ));
    if let Some(intent) = artifact.intent.as_ref() {
        match intent {
            floe_core::IntentInput::Structured(s) => {
                out.push_str("Stated intent (structured):\n");
                if !s.title.is_empty() {
                    out.push_str(&format!("  title: {}\n", s.title));
                }
                if !s.summary.is_empty() {
                    out.push_str(&format!("  summary: {}\n", s.summary));
                }
                for c in &s.claims {
                    out.push_str(&format!("  - {}\n", c.statement));
                }
                out.push('\n');
            }
            floe_core::IntentInput::RawText(raw) => {
                out.push_str("Stated intent (raw):\n");
                out.push_str(raw);
                out.push_str("\n\n");
            }
        }
    }
    out.push_str("Seed entities (the hunks point at these):\n");
    for e in &flow.entities {
        out.push_str(&format!("  - {e}\n"));
    }
    out.push('\n');
    out.push_str("Hunks in this flow:\n");
    for hid in &flow.hunk_ids {
        if let Some(h) = artifact.hunks.iter().find(|x| &x.id == hid) {
            out.push_str(&format!("  - {hid}: {}\n", hunk_kind_name(h)));
        }
    }
    out.push('\n');
    out.push_str(
        "Investigate via tools, then emit the JSON per the system \
         prompt. Nothing else.",
    );
    out
}

fn hunk_kind_name(h: &floe_core::Hunk) -> &'static str {
    match &h.kind {
        floe_core::HunkKind::Call { .. } => "call",
        floe_core::HunkKind::State { .. } => "state",
        floe_core::HunkKind::Api { .. } => "api",
        floe_core::HunkKind::Lock { .. } => "lock",
        floe_core::HunkKind::Data { .. } => "data",
        floe_core::HunkKind::Docs { .. } => "docs",
        floe_core::HunkKind::Deletion { .. } => "deletion",
    }
}

fn build_tool_defs(specs: &[ToolSpec]) -> Vec<ToolDef> {
    specs
        .iter()
        .filter(|s| MEMBERSHIP_TOOLS.contains(&s.name.as_str()))
        .map(|s| ToolDef {
            kind: "function",
            function: ToolDefFunction {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.input_schema.clone(),
            },
        })
        .collect()
}
