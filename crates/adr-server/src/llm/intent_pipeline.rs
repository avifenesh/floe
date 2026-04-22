//! Intent-fit + proof-verification LLM passes.
//!
//! Per flow, two GLM sessions (by default — local models are permitted
//! but capped per `feedback_proof_uses_glm.md`):
//!
//! 1. **Intent-fit** — does this flow deliver a claim the PR's stated
//!    intent makes? Emits [`IntentFit`]. ~10 tool-call budget.
//! 2. **Proof-verification** — is there real evidence for the flow's
//!    plausible claims (benchmarks, examples exercising the claim,
//!    tests asserting the specific claim)? Emits [`Proof`]. ~15
//!    tool-call budget (evidence-hunting needs grep + read round-trips).
//!
//! Both sessions share the same rendered context (intent + flow +
//! notes) and the same MCP toolbox (`adr.read_file` / `adr.grep` /
//! `adr.glob` + `adr.get_entity` / `adr.neighbors`). They run
//! sequentially per flow; across flows they run in parallel.
//!
//! Output is [`IntentOutcome`] — a per-flow collection the worker
//! merges back into the artifact.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use adr_core::{
    intent::{Intent, IntentFit, IntentInput, Proof},
    Artifact, Flow, Side,
};
use anyhow::{anyhow, Context, Result};
use futures::future::join_all;
use serde_json::{json, Value};

use super::config::{LlmConfig, LlmProvider};
use super::glm_client::GlmClient;
use super::mcp_client::{McpClient, ToolSpec};
use super::ollama_client::{
    ChatMessage, ChatRequest, OllamaClient, ToolDef as OllamaToolDef,
    ToolDefFunction as OllamaToolDefFunction,
};

/// Max tool calls per intent-fit session, rendered into the prompt.
/// Intent-fit is a small-scope judgement from the flow's hunks plus
/// a few targeted lookups — 15 tools is plenty.
const INTENT_FIT_MAX_TOOL_CALLS: u32 = 15;
/// Max tool calls per proof-verification session, rendered into the
/// prompt. Proof-verification on an N-claim intent is real agentic
/// work — glob candidate files, read several, grep for claim terms,
/// read tests, cross-reference implementations. 9 claims × 3–4 tool
/// calls = ~30 realistic upper bound. Earlier caps of 15 were the
/// primary cause of "session ended without a final assistant
/// message" — the model hit turn cap mid-hunt with no budget to
/// actually answer.
const PROOF_MAX_TOOL_CALLS: u32 = 30;

/// Hard session-turn caps per pass. Slightly higher than the tool
/// budget because the model occasionally batches multiple tools in
/// one turn, and we still need room for the final content turn.
const INTENT_FIT_MAX_TURNS: u32 = 18;
const PROOF_MAX_TURNS: u32 = 34;

/// Tools the intent + proof passes are allowed to call. Filtered from
/// the MCP child's `tools/list` so the model never sees mutation
/// tools (it can't propose flows from inside a proof session).
const PROOF_SAFE_TOOLS: &[&str] = &[
    "adr.get_entity",
    "adr.neighbors",
    "adr.list_entities",
    "adr.read_file",
    "adr.grep",
    "adr.glob",
];

#[derive(Debug)]
pub struct IntentPipeline<'a> {
    pub proof_cfg: &'a LlmConfig,
    /// Head-snapshot root — the fs tools resolve paths against this.
    pub repo_root: &'a Path,
    pub intent_fit_version: &'a str,
    pub proof_version: &'a str,
}

#[derive(Debug, Clone)]
pub struct IntentOutcome {
    pub per_flow: Vec<PerFlowResult>,
    pub intent_fit_version: String,
    pub proof_version: String,
    pub model: String,
    /// LLM-generated 1–2 sentence summary of the stated intent. `None`
    /// when the intent arrived pre-structured or the summarising call
    /// errored (worker leaves the existing field unchanged).
    pub intent_summary: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PerFlowResult {
    pub flow_id: String,
    pub intent_fit: Option<IntentFit>,
    pub proof: Option<Proof>,
    /// Any errors from either pass — the worker surfaces them so a
    /// partial proof run is still useful. Format: `"intent-fit: <msg>"`
    /// / `"proof: <msg>"`.
    pub errors: Vec<String>,
}

impl<'a> IntentPipeline<'a> {
    pub async fn run(&self, artifact: &Artifact) -> Result<IntentOutcome> {
        let intent = artifact
            .intent
            .as_ref()
            .ok_or_else(|| anyhow!("intent pipeline invoked with no intent on artifact"))?;
        let intent_rendered = render_intent(intent);

        // Summarize the raw PR body once before fan-out — gives the
        // reviewer a 1–2 sentence read of "what this PR claims to do"
        // without parsing the full description. Per-flow proof passes
        // still get the original (unsummarised) intent so claims aren't
        // diluted. Best-effort: failure logs and we leave the field
        // unset rather than aborting the whole proof pass.
        let intent_summary = match intent {
            IntentInput::RawText(raw) => match summarize_intent(self.proof_cfg, raw).await {
                Ok(s) => Some(s),
                Err(e) => {
                    tracing::warn!(error = %e, "intent summarization failed; continuing without summary");
                    None
                }
            },
            IntentInput::Structured(_) => None,
        };

        let side_only = artifact.side_only(Side::Head);
        let tmp_artifact = write_tmp_artifact(&side_only)?;

        let intent_fit_prompt = load_prompt(
            "intent_fit",
            self.intent_fit_version,
            INTENT_FIT_MAX_TOOL_CALLS,
        )?;
        let proof_prompt =
            load_prompt("proof_verification", self.proof_version, PROOF_MAX_TOOL_CALLS)?;

        tracing::info!(
            flows = artifact.flows.len(),
            model = %self.proof_cfg.model,
            intent_fit_version = %self.intent_fit_version,
            proof_version = %self.proof_version,
            "intent + proof pipeline starting (parallel per-flow + per-pass)"
        );

        // Build per-flow futures; join_all runs them concurrently.
        // Inside each flow, the two passes (intent-fit + proof) run
        // concurrently via tokio::join! — every session spawns its
        // own MCP child so there's no shared-state contention.
        let tmp_artifact = Arc::new(tmp_artifact);
        let intent_fit_prompt = Arc::new(intent_fit_prompt);
        let proof_prompt = Arc::new(proof_prompt);
        let proof_cfg = Arc::new(self.proof_cfg.clone());
        let repo_root: Arc<Path> = Arc::from(self.repo_root.to_path_buf().into_boxed_path());
        let intent_fit_version: Arc<str> = Arc::from(self.intent_fit_version);
        let proof_version: Arc<str> = Arc::from(self.proof_version);

        let flow_futures = artifact.flows.iter().map(|flow| {
            let flow_rendered = render_flow(artifact, flow);
            let notes_rendered = if artifact.notes.trim().is_empty() {
                "(no reviewer notes supplied)".to_string()
            } else {
                artifact.notes.clone()
            };
            let user_message = format!(
                "intent:\n{intent_rendered}\n\nflow:\n{flow_rendered}\n\nnotes:\n{notes_rendered}\n",
            );
            let flow_id = flow.id.clone();
            let flow_name = flow.name.clone();
            let tmp = Arc::clone(&tmp_artifact);
            let ifp = Arc::clone(&intent_fit_prompt);
            let pp = Arc::clone(&proof_prompt);
            let cfg = Arc::clone(&proof_cfg);
            let root = Arc::clone(&repo_root);
            let ifv = Arc::clone(&intent_fit_version);
            let pv = Arc::clone(&proof_version);
            async move {
                tracing::info!(flow_id = %flow_id, flow_name = %flow_name, "starting per-flow sessions");
                let um = user_message;
                let (fit_res, proof_res) = tokio::join!(
                    run_session(
                        cfg.as_ref(),
                        root.as_ref(),
                        tmp.as_ref(),
                        ifp.as_str(),
                        &um,
                        INTENT_FIT_MAX_TURNS,
                    ),
                    run_session(
                        cfg.as_ref(),
                        root.as_ref(),
                        tmp.as_ref(),
                        pp.as_str(),
                        &um,
                        PROOF_MAX_TURNS,
                    ),
                );
                let mut errors: Vec<String> = Vec::new();
                let intent_fit = match fit_res {
                    Ok(v) => match parse_intent_fit(v, &cfg.model, ifv.as_ref()) {
                        Ok(f) => Some(f),
                        Err(e) => {
                            errors.push(format!("intent-fit parse: {e}"));
                            None
                        }
                    },
                    Err(e) => {
                        errors.push(format!("intent-fit: {e}"));
                        None
                    }
                };
                let proof = match proof_res {
                    Ok(v) => match parse_proof(v, &cfg.model, pv.as_ref()) {
                        Ok(p) => Some(p),
                        Err(e) => {
                            errors.push(format!("proof parse: {e}"));
                            None
                        }
                    },
                    Err(e) => {
                        errors.push(format!("proof: {e}"));
                        None
                    }
                };
                tracing::info!(
                    flow_id = %flow_id,
                    intent_fit = ?intent_fit.as_ref().map(|f| f.verdict),
                    proof = ?proof.as_ref().map(|p| p.verdict),
                    "per-flow sessions complete"
                );
                PerFlowResult {
                    flow_id,
                    intent_fit,
                    proof,
                    errors,
                }
            }
        });
        let per_flow: Vec<PerFlowResult> = join_all(flow_futures).await;
        let _ = std::fs::remove_file(tmp_artifact.as_ref());

        Ok(IntentOutcome {
            per_flow,
            intent_fit_version: self.intent_fit_version.to_string(),
            proof_version: self.proof_version.to_string(),
            model: self.proof_cfg.model.clone(),
            intent_summary,
        })
    }

    // `run_session` moved to a free async fn (below) so it can be
    // polled concurrently from multiple per-flow futures without
    // borrowing `&self`.
}

/// One-shot LLM call to compress a raw PR description into 1–2
/// sentences. No tools, no agentic loop — single prompt, single
/// response. Uses the same `LlmConfig` as the proof pass so we don't
/// add a new env knob; respects the GLM concurrency semaphore via
/// `GlmClient::chat`. Returns a trimmed summary or an error.
async fn summarize_intent(cfg: &LlmConfig, raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(anyhow!("raw intent is empty"));
    }
    let system = "You are a senior reviewer. Read the PR description and reply with ONE OR TWO short sentences (≤60 words total) capturing only the stated intent — what the PR claims to deliver. No bullet points, no quoting the description, no commentary on quality. Plain prose only.";
    let user = format!("PR description:\n\n{trimmed}");
    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: system.into(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: user,
                tool_calls: Vec::new(),
                tool_name: None,
            },
        ],
        tools: Vec::new(),
        stream: false,
        options: Some(json!({ "temperature": 0.3, "num_predict": 200 })),
        keep_alive: None,
    };

    let content = match cfg.provider {
        LlmProvider::Glm => {
            let key = cfg
                .api_key
                .clone()
                .ok_or_else(|| anyhow!("ADR_GLM_API_KEY required for intent summarization"))?;
            let client = GlmClient::new(cfg.base_url.clone(), key);
            let resp = client.chat(req).await.context("glm summarize_intent")?;
            resp.message.content
        }
        LlmProvider::Ollama => {
            let client = OllamaClient::new(cfg.base_url.clone());
            let resp = client.chat(req).await.context("ollama summarize_intent")?;
            resp.message.content
        }
    };
    let cleaned = content.trim();
    if cleaned.is_empty() {
        return Err(anyhow!("model returned empty summary"));
    }
    Ok(cleaned.to_string())
}

/// Run one agentic session. `system_prompt` is the pre-rendered
/// prompt body; `user_message` is the concrete intent + flow +
/// notes payload. Returns the parsed JSON from the model's final
/// message. Tool calls dispatch through a freshly-spawned MCP child.
async fn run_session(
    proof_cfg: &LlmConfig,
    repo_root: &Path,
    artifact_path: &Path,
    system_prompt: &str,
    user_message: &str,
    max_turns: u32,
) -> Result<Value> {
    {
        tracing::info!(
            repo_root = %repo_root.display(),
            "spawning adr-mcp (proof mode) for session"
        );
        let mut mcp = McpClient::spawn_proof(
            artifact_path,
            &proof_cfg.model,
            "intent-proof",
            repo_root,
        )
        .await
        .context("spawning adr-mcp in proof mode")?;
        mcp.initialize().await.context("mcp initialize")?;
        let tool_specs = mcp.list_tools().await.context("mcp tools/list")?;
        let ollama_tools = build_tool_defs(&tool_specs);

        let backing = make_backing_client(proof_cfg);
        let options = provider_options(proof_cfg);

        let mut messages: Vec<ChatMessage> = vec![
            ChatMessage {
                role: "system".into(),
                content: system_prompt.to_string(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
            ChatMessage {
                role: "user".into(),
                content: user_message.to_string(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
        ];

        let mut final_text: Option<String> = None;
        // Nudge budget — model sometimes returns an empty turn (no
        // content + no tool calls) when it's ready to answer but
        // hasn't emitted the JSON. Allow one explicit "emit JSON
        // now" nudge before bailing; more than that is a model
        // that's genuinely stuck.
        let mut nudges_sent: u32 = 0;
        const MAX_NUDGES: u32 = 1;
        // Turn-budget landmarks proportional to the per-pass cap:
        //   warn  at  max_turns - 4  → "wrap it up, 3 turns left"
        //   final at  max_turns - 2  → strip tools, force content
        // Leaving 1 spare turn after "final" gives the model room to
        // actually emit content after we stop accepting tool calls.
        // On intent-fit (18 turns) that's warn@14 / final@16.
        // On proof (34 turns) that's warn@30 / final@32.
        let budget_warn_turn = max_turns.saturating_sub(4);
        let budget_final_turn = max_turns.saturating_sub(2);
        let mut budget_warned = false;
        let mut budget_finalized = false;
        for turn in 0..max_turns {
            tracing::debug!(turn, "glm chat turn");
            // Budget nudge landmarks: warn early, force emit late.
            if turn >= budget_warn_turn && !budget_warned {
                tracing::info!(turn, "injecting budget-warn nudge");
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: format!(
                        "Budget warning: you've used {} tool-call turns. You have {} turns left. \
                         Finish any last investigation and emit the final JSON verdict soon.",
                        turn,
                        max_turns - turn
                    ),
                    tool_calls: Vec::new(),
                    tool_name: None,
                });
                budget_warned = true;
            }
            let tools_for_this_turn = if turn >= budget_final_turn && !budget_finalized {
                tracing::info!(turn, "injecting budget-final nudge and dropping tools");
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: "BUDGET EXHAUSTED. No more tool calls. Emit the final JSON \
                         object now — exactly one JSON matching the schema in the \
                         system prompt, no prose, no code fence."
                        .into(),
                    tool_calls: Vec::new(),
                    tool_name: None,
                });
                budget_finalized = true;
                // Strip tools so the model physically cannot call one;
                // forces it to produce content.
                Vec::new()
            } else {
                ollama_tools.clone()
            };
            let req = ChatRequest {
                model: proof_cfg.model.clone(),
                messages: messages.clone(),
                tools: tools_for_this_turn,
                stream: false,
                options: options.clone(),
                keep_alive: Some(proof_cfg.keep_alive.clone()),
            };
            let resp = match backing.chat(req).await {
                Ok(r) => r,
                Err(e) => {
                    let _ = mcp.shutdown().await;
                    // Use `{e:#}` so the full anyhow context chain is
                    // captured — a bare `{e}` loses underlying causes
                    // (reqwest error reasons, 4xx response bodies,
                    // breaker-refused messages).
                    return Err(anyhow!("LLM chat failed: {e:#}"));
                }
            };
            let msg = resp.message;
            messages.push(msg.clone());

            if !msg.tool_calls.is_empty() {
                // Dispatch each tool call, append the result as a role=tool message.
                for call in &msg.tool_calls {
                    let name = call.function.name.clone();
                    tracing::info!(tool = %name, "proof tool call");
                    if !PROOF_SAFE_TOOLS.iter().any(|&t| t == name) {
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: format!(
                                "ERROR: tool `{name}` is not available in this session"
                            ),
                            tool_calls: Vec::new(),
                            tool_name: Some(name),
                        });
                        continue;
                    }
                    let args = call.function.arguments.clone();
                    let result = match mcp.call_tool(&name, args).await {
                        Ok(r) => r.text,
                        Err(e) => format!("ERROR: mcp call_tool({name}): {e}"),
                    };
                    messages.push(ChatMessage {
                        role: "tool".into(),
                        content: result,
                        tool_calls: Vec::new(),
                        tool_name: Some(name),
                    });
                }
                continue; // next turn — model processes tool results
            }

            // No tool calls → assistant message is *maybe* the final
            // answer. GLM-4.6/4.7 ship a non-standard XML tool-call
            // template (`<tool_call>name<arg_key>k</arg_key><arg_value>
            // v</arg_value>...</tool_call>`) and occasionally leak it
            // into `content` when the OpenAI-compatible parser on the
            // server side misses a turn (see `project_glm_tool_call_drift.md`).
            // Research consensus: client-side parse + dispatch beats
            // corrective nudging (vLLM / SGLang / mlx-lm all do this).
            // Try to rehydrate the tool calls from content; if that
            // works, treat the turn as a normal tool-call turn.
            if !msg.content.trim().is_empty() {
                let rehydrated = parse_inline_glm_tool_calls(&msg.content);
                if !rehydrated.is_empty() {
                    tracing::info!(
                        count = rehydrated.len(),
                        "rehydrated {} inline tool call(s) from content",
                        rehydrated.len()
                    );
                    for (name, args) in &rehydrated {
                        tracing::info!(tool = %name, "proof tool call (rehydrated)");
                        if !PROOF_SAFE_TOOLS.iter().any(|&t| t == name) {
                            messages.push(ChatMessage {
                                role: "tool".into(),
                                content: format!(
                                    "ERROR: tool `{name}` is not available in this session"
                                ),
                                tool_calls: Vec::new(),
                                tool_name: Some(name.clone()),
                            });
                            continue;
                        }
                        let result = match mcp.call_tool(name, args.clone()).await {
                            Ok(r) => r.text,
                            Err(e) => format!("ERROR: mcp call_tool({name}): {e}"),
                        };
                        messages.push(ChatMessage {
                            role: "tool".into(),
                            content: result,
                            tool_calls: Vec::new(),
                            tool_name: Some(name.clone()),
                        });
                    }
                    continue; // next turn — model processes tool results
                }
                final_text = Some(msg.content);
                break;
            }
            // Empty content + no tool calls. Nudge the model once
            // to emit the JSON verdict; GLM occasionally emits a
            // blank closing turn after its last tool call. A second
            // empty turn means the model is genuinely stuck — bail
            // rather than wasting budget.
            if nudges_sent >= MAX_NUDGES {
                tracing::warn!(
                    "empty turn after {} nudge(s) — bailing",
                    nudges_sent
                );
                break;
            }
            tracing::info!(
                "empty turn — nudging model to emit final JSON"
            );
            messages.push(ChatMessage {
                role: "user".into(),
                content:
                    "Emit the final JSON verdict now. No more tool calls. \
                     Reply with exactly one JSON object matching the schema \
                     in the system prompt — no prose, no code fence, nothing \
                     else."
                        .into(),
                tool_calls: Vec::new(),
                tool_name: None,
            });
            nudges_sent += 1;
            continue;
        }
        let _ = mcp.shutdown().await;

        let text =
            final_text.ok_or_else(|| anyhow!("session ended without a final assistant message"))?;
        extract_json(&text).ok_or_else(|| anyhow!("no JSON object in final message: {text}"))
    }
}

fn make_backing_client(cfg: &LlmConfig) -> BackingClient {
    match cfg.provider {
        LlmProvider::Ollama => BackingClient::Ollama(OllamaClient::new(cfg.base_url.clone())),
        LlmProvider::Glm => BackingClient::Glm(GlmClient::new(
            cfg.base_url.clone(),
            cfg.api_key.clone().unwrap_or_default(),
        )),
    }
}

/// Provider-specific `options` block matching what the probe pipeline
/// passes. Temperature low, num_predict generous so multi-tool turns
/// don't get truncated mid-JSON.
fn provider_options(cfg: &LlmConfig) -> Option<Value> {
    match cfg.provider {
        LlmProvider::Ollama => Some(json!({
            "num_ctx": cfg.num_ctx,
            "num_predict": cfg.num_predict,
            "temperature": cfg.temperature,
        })),
        LlmProvider::Glm => Some(json!({
            "max_tokens": cfg.num_predict,
            "temperature": cfg.temperature,
        })),
    }
}

enum BackingClient {
    Ollama(OllamaClient),
    Glm(GlmClient),
}

impl BackingClient {
    async fn chat(&self, req: ChatRequest) -> Result<super::ollama_client::ChatResponse> {
        match self {
            BackingClient::Ollama(c) => c.chat(req).await,
            BackingClient::Glm(c) => c.chat(req).await,
        }
    }
}

fn build_tool_defs(specs: &[ToolSpec]) -> Vec<OllamaToolDef> {
    specs
        .iter()
        .filter(|s| PROOF_SAFE_TOOLS.contains(&s.name.as_str()))
        .map(|s| OllamaToolDef {
            kind: "function",
            function: OllamaToolDefFunction {
                name: s.name.clone(),
                description: s.description.clone(),
                parameters: s.input_schema.clone(),
            },
        })
        .collect()
}

// ─────────────────────────────────────────────────────────────────────
// Prompt loading
// ─────────────────────────────────────────────────────────────────────

fn load_prompt(name: &str, version: &str, max_tool_calls: u32) -> Result<String> {
    let root = repo_root()?;
    let path = root
        .join("prompts")
        .join(name)
        .join(version)
        .join(format!("{name}.md"));
    let raw = std::fs::read_to_string(&path)
        .with_context(|| format!("reading prompt {}", path.display()))?;
    Ok(raw.replace("{{max_tool_calls}}", &max_tool_calls.to_string()))
}

fn repo_root() -> Result<PathBuf> {
    if let Ok(r) = std::env::var("ADR_REPO_ROOT") {
        return Ok(PathBuf::from(r));
    }
    let start = std::env::current_dir()?;
    let mut cur: &Path = &start;
    loop {
        let candidate = cur.join("Cargo.toml");
        if candidate.is_file() {
            if let Ok(s) = std::fs::read_to_string(&candidate) {
                if s.contains("[workspace]") {
                    return Ok(cur.to_path_buf());
                }
            }
        }
        match cur.parent() {
            Some(p) => cur = p,
            None => {
                return Err(anyhow!(
                    "could not locate repo root from {}",
                    start.display()
                ));
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Rendering helpers — intent + flow → reviewer-readable text
// ─────────────────────────────────────────────────────────────────────

fn render_intent(input: &IntentInput) -> String {
    match input {
        IntentInput::Structured(i) => render_structured_intent(i),
        IntentInput::RawText(s) => format!(
            "(raw text intent — synthesise claims if no structure below)\n{}",
            s.trim()
        ),
    }
}

fn render_structured_intent(i: &Intent) -> String {
    let mut out = String::new();
    out.push_str(&format!("  title: {}\n", i.title));
    if !i.summary.is_empty() {
        out.push_str(&format!("  summary: {}\n", i.summary));
    }
    if !i.claims.is_empty() {
        out.push_str("  claims:\n");
        for (idx, c) in i.claims.iter().enumerate() {
            let kind = match c.evidence_type {
                adr_core::EvidenceType::Bench => "bench",
                adr_core::EvidenceType::Example => "example",
                adr_core::EvidenceType::Test => "test",
                adr_core::EvidenceType::Observation => "observation",
            };
            let detail = if c.detail.is_empty() {
                String::new()
            } else {
                format!(" — {}", c.detail)
            };
            out.push_str(&format!(
                "    {idx}. [{kind}] {}{detail}\n",
                c.statement
            ));
        }
    }
    out
}

fn render_flow(artifact: &Artifact, flow: &Flow) -> String {
    let mut out = String::new();
    out.push_str(&format!("  id: {}\n", flow.id));
    out.push_str(&format!("  name: {}\n", flow.name));
    out.push_str(&format!("  rationale: {}\n", flow.rationale));
    if !flow.entities.is_empty() {
        out.push_str(&format!("  entities: {}\n", flow.entities.join(", ")));
    }
    if !flow.extra_entities.is_empty() {
        out.push_str(&format!(
            "  extra_entities: {}\n",
            flow.extra_entities.join(", ")
        ));
    }
    if !flow.hunk_ids.is_empty() {
        out.push_str("  hunks:\n");
        for hid in &flow.hunk_ids {
            let Some(h) = artifact.hunks.iter().find(|h| &h.id == hid) else {
                out.push_str(&format!("    - {hid} (missing)\n"));
                continue;
            };
            let kind = hunk_kind_name(h);
            out.push_str(&format!("    - {hid} ({kind})\n"));
        }
    }
    out
}

fn hunk_kind_name(h: &adr_core::Hunk) -> &'static str {
    match h.kind {
        adr_core::HunkKind::Call { .. } => "call",
        adr_core::HunkKind::State { .. } => "state",
        adr_core::HunkKind::Api { .. } => "api",
    }
}

// ─────────────────────────────────────────────────────────────────────
// JSON extraction + parsing
// ─────────────────────────────────────────────────────────────────────

// `parse_inline_glm_tool_calls` moved to `super::tool_call_drift` so
// synth, intent, and proof share one implementation. Re-exported
// from there for the test module below.
pub(crate) use super::tool_call_drift::parse_inline_glm_tool_calls;

/// Extract the first balanced JSON object from a message. Models
/// sometimes wrap the answer in code fences or add prose around it;
/// we find the first `{` and the matching `}` by balanced-brace scan.
pub(crate) fn extract_json(s: &str) -> Option<Value> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escape = false;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if escape {
            escape = false;
            continue;
        }
        if in_string {
            if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_string = false;
            }
            continue;
        }
        match c {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(i);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    let from = start?;
                    let slice = &s[from..=i];
                    return serde_json::from_str(slice).ok();
                }
            }
            _ => {}
        }
    }
    None
}

fn parse_intent_fit(v: Value, model: &str, version: &str) -> Result<IntentFit> {
    use adr_core::{intent::IntentFitVerdict, Strength};
    let verdict = str_field(&v, "verdict")?;
    let verdict = match verdict.as_str() {
        "delivers" => IntentFitVerdict::Delivers,
        "partial" => IntentFitVerdict::Partial,
        "unrelated" => IntentFitVerdict::Unrelated,
        "no-intent" => IntentFitVerdict::NoIntent,
        other => return Err(anyhow!("unknown verdict: {other}")),
    };
    let strength = parse_strength(&v)?;
    let reasoning = str_field(&v, "reasoning").unwrap_or_default();
    let matched_claims: Vec<usize> = v
        .get("matched_claims")
        .and_then(|x| x.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|n| n.as_u64().map(|n| n as usize))
                .collect()
        })
        .unwrap_or_default();
    let _ = Strength::Low; // witness used
    Ok(IntentFit {
        verdict,
        strength,
        reasoning,
        matched_claims,
        model: model.to_string(),
        prompt_version: version.to_string(),
    })
}

fn parse_proof(v: Value, model: &str, version: &str) -> Result<Proof> {
    use adr_core::intent::{ClaimProofKind, ClaimProofStatus, EvidenceType, ProofEvidence, ProofVerdict};
    let verdict = str_field(&v, "verdict")?;
    let verdict = match verdict.as_str() {
        "strong" => ProofVerdict::Strong,
        "partial" => ProofVerdict::Partial,
        "missing" => ProofVerdict::Missing,
        "no-intent" => ProofVerdict::NoIntent,
        other => return Err(anyhow!("unknown proof verdict: {other}")),
    };
    let strength = parse_strength(&v)?;
    let reasoning = str_field(&v, "reasoning").unwrap_or_default();
    let mut claims: Vec<ClaimProofStatus> = Vec::new();
    if let Some(arr) = v.get("claims").and_then(|x| x.as_array()) {
        for item in arr {
            let claim_index: i32 = item
                .get("claim_index")
                .and_then(|n| n.as_i64())
                .map(|n| n as i32)
                .unwrap_or(-1);
            let statement = item
                .get("statement")
                .and_then(|n| n.as_str())
                .unwrap_or_default()
                .to_string();
            let status = match item.get("status").and_then(|n| n.as_str()).unwrap_or("") {
                "found" => ClaimProofKind::Found,
                "partial" => ClaimProofKind::Partial,
                "missing" => ClaimProofKind::Missing,
                other => return Err(anyhow!("unknown claim status: {other}")),
            };
            let c_strength = parse_strength(item)?;
            let mut evidence: Vec<ProofEvidence> = Vec::new();
            if let Some(ev_arr) = item.get("evidence").and_then(|n| n.as_array()) {
                for ev in ev_arr {
                    let evidence_type = match ev
                        .get("evidence_type")
                        .and_then(|n| n.as_str())
                        .unwrap_or("")
                    {
                        "bench" => EvidenceType::Bench,
                        "example" => EvidenceType::Example,
                        "test" => EvidenceType::Test,
                        "observation" => EvidenceType::Observation,
                        other => {
                            return Err(anyhow!("unknown evidence_type: {other}"));
                        }
                    };
                    let detail = ev
                        .get("detail")
                        .and_then(|n| n.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let path = ev
                        .get("path")
                        .and_then(|n| n.as_str())
                        .map(|s| s.to_string());
                    evidence.push(ProofEvidence {
                        evidence_type,
                        detail,
                        path,
                    });
                }
            }
            claims.push(ClaimProofStatus {
                claim_index,
                statement,
                status,
                evidence,
                strength: c_strength,
            });
        }
    }
    Ok(Proof {
        verdict,
        strength,
        reasoning,
        claims,
        model: model.to_string(),
        prompt_version: version.to_string(),
    })
}

fn parse_strength(v: &Value) -> Result<adr_core::Strength> {
    use adr_core::Strength;
    match v.get("strength").and_then(|n| n.as_str()).unwrap_or("low") {
        "high" => Ok(Strength::High),
        "medium" => Ok(Strength::Medium),
        "low" => Ok(Strength::Low),
        other => Err(anyhow!("unknown strength: {other}")),
    }
}

fn str_field(v: &Value, name: &str) -> Result<String> {
    Ok(v.get(name)
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("missing string field `{name}`"))?
        .to_string())
}

fn write_tmp_artifact(artifact: &Artifact) -> Result<PathBuf> {
    let dir = std::env::temp_dir();
    let fname = format!("adr-intent-{}.json", uuid::Uuid::new_v4());
    let path = dir.join(fname);
    let bytes = serde_json::to_vec(artifact)?;
    std::fs::write(&path, &bytes)
        .with_context(|| format!("writing side-only artifact to {}", path.display()))?;
    Ok(path)
}

// Unused-imports sweep — keep Duration import for the timeout if we
// add one later; silence clippy for now.
#[allow(dead_code)]
fn _touch_duration() -> Duration {
    Duration::from_secs(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_strips_prose_prefix() {
        let s = "here's my verdict\n{\"verdict\":\"delivers\",\"strength\":\"high\",\"reasoning\":\"r\",\"matched_claims\":[0]}\n";
        let v = extract_json(s).expect("should extract");
        assert_eq!(v.get("verdict").unwrap().as_str(), Some("delivers"));
    }

    #[test]
    fn extract_json_handles_nested_objects() {
        let s = "{\"a\":{\"b\":{\"c\":1}},\"d\":2}";
        let v = extract_json(s).expect("nested");
        assert_eq!(v.get("d").unwrap().as_u64(), Some(2));
    }

    #[test]
    fn extract_json_ignores_braces_inside_strings() {
        let s = "blob { \"text\":\"a }{ b\",\"n\":42 } trailing";
        let v = extract_json(s).expect("should extract");
        assert_eq!(v.get("n").unwrap().as_u64(), Some(42));
    }

    #[test]
    fn parse_intent_fit_accepts_minimal_shape() {
        let v = serde_json::json!({
            "verdict":"delivers","strength":"high","reasoning":"ok","matched_claims":[0,1]
        });
        let got = parse_intent_fit(v, "glm-4.7", "v0.1.0").unwrap();
        assert_eq!(got.matched_claims, vec![0, 1]);
        assert_eq!(got.model, "glm-4.7");
    }

    #[test]
    fn parse_inline_glm_tool_calls_single() {
        let content = "Let me check: <tool_call>adr.read_file<arg_key>file_path</arg_key><arg_value>src/foo.ts</arg_value></tool_call>";
        let got = parse_inline_glm_tool_calls(content);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].0, "adr.read_file");
        assert_eq!(got[0].1.get("file_path").unwrap().as_str(), Some("src/foo.ts"));
    }

    #[test]
    fn parse_inline_glm_tool_calls_multiple_with_int_and_bool_coercion() {
        let content = "<tool_call>adr.grep<arg_key>pattern</arg_key><arg_value>foo</arg_value><arg_key>limit</arg_key><arg_value>30</arg_value><arg_key>case_insensitive</arg_key><arg_value>true</arg_value></tool_call><tool_call>adr.glob<arg_key>pattern</arg_key><arg_value>**/*.ts</arg_value></tool_call>";
        let got = parse_inline_glm_tool_calls(content);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].0, "adr.grep");
        assert_eq!(got[0].1.get("limit").unwrap().as_i64(), Some(30));
        assert_eq!(got[0].1.get("case_insensitive").unwrap().as_bool(), Some(true));
        assert_eq!(got[1].0, "adr.glob");
    }

    #[test]
    fn parse_inline_glm_tool_calls_ignores_plain_prose() {
        let content = "No tool calls here, just reasoning about <tool_call> as a concept.";
        let got = parse_inline_glm_tool_calls(content);
        // Opens but never closes → parser bails cleanly, returns empty.
        assert!(got.is_empty());
    }

    #[test]
    fn parse_proof_accepts_full_shape() {
        let v = serde_json::json!({
            "verdict":"partial","strength":"medium","reasoning":"r",
            "claims":[
                {"claim_index":0,"statement":"p99 drop","status":"found","strength":"high",
                 "evidence":[{"evidence_type":"bench","detail":"180→72ms"}]},
                {"claim_index":1,"statement":"no regressions","status":"missing","strength":"low","evidence":[]}
            ]
        });
        let got = parse_proof(v, "glm-4.7", "v0.1.0").unwrap();
        assert_eq!(got.claims.len(), 2);
        assert_eq!(got.claims[0].evidence.len(), 1);
        assert_eq!(got.claims[1].evidence.len(), 0);
    }
}
