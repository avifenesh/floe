//! The agent loop — shuttles tool calls between Ollama and `floe-mcp`
//! until the model calls `floe.finalize` (or we hit a hard bound).

use std::path::Path;

use floe_core::Flow;
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};

use super::config::{LlmConfig, LlmProvider};
use super::glm_client::GlmClient;
use super::mcp_client::{McpClient, ToolCallResult};
use super::tool_call_drift::{coerce_malformed_arguments, parse_inline_glm_tool_calls};
use super::ollama_client::{
    ChatMessage, ChatRequest, ChatResponse, OllamaClient, ToolCall, ToolDef,
};
use super::prompt::{self, PromptInputs};

/// Hard cap on LLM turns. Each turn = one Ollama chat round, which may
/// emit several parallel tool calls. Complements (not replaces) the
/// per-tool-call budget enforced by the MCP server.
const MAX_TURNS: u32 = 40;

/// If the model returns no tool calls this many turns in a row, stop.
/// Empty-response stall is usually Gemma+Ollama losing the tool-call
/// schema after long histories — restarting the pipeline is cheaper
/// than burning the whole turn budget on dead turns.
const MAX_CONSECUTIVE_EMPTY: u32 = 3;

pub enum SynthesisOutcome {
    Accepted(Vec<Flow>),
    /// The LLM finalized but the host rejected on invariants.
    Rejected { rule: String, detail: String },
    /// The LLM ran out of turns without calling `floe.finalize`.
    NoFinalize,
    /// Something below the loop failed (process spawn, HTTP, parsing).
    Errored(String),
}

impl SynthesisOutcome {
    pub fn as_flows(&self) -> Option<&[Flow]> {
        match self {
            Self::Accepted(f) => Some(f.as_slice()),
            _ => None,
        }
    }
}

pub async fn synthesize(
    artifact_path: &Path,
    hunk_count: usize,
    initial_cluster_count: usize,
    cfg: &LlmConfig,
) -> SynthesisOutcome {
    match run(artifact_path, hunk_count, initial_cluster_count, cfg).await {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!(error = %e, "LLM synthesis errored");
            SynthesisOutcome::Errored(format!("{e:#}"))
        }
    }
}

async fn run(
    artifact_path: &Path,
    hunk_count: usize,
    initial_cluster_count: usize,
    cfg: &LlmConfig,
) -> Result<SynthesisOutcome> {
    // 1. Spawn the MCP child and handshake.
    let mut mcp = McpClient::spawn(
        artifact_path,
        &format!("{}:{}", cfg.provider, cfg.model),
        env!("CARGO_PKG_VERSION"),
    )
    .await?;
    mcp.initialize().await.context("mcp initialize")?;
    let tool_specs = mcp.list_tools().await.context("mcp tools/list")?;
    // v0.3.0 prompt tells the model not to call list_hunks /
    // list_flows_initial — the host supplies their results up front.
    // We still expose them in the tool schema so the MCP child remains a
    // standalone, generic server (same tools as any external client).
    let tools: Vec<ToolDef> = tool_specs.iter().map(OllamaClient::tool_def_from_mcp).collect();
    tracing::info!(
        tool_count = tools.len(),
        num_ctx = cfg.num_ctx,
        num_predict = cfg.num_predict,
        temperature = cfg.temperature,
        keep_alive = %cfg.keep_alive,
        "floe-mcp child ready"
    );

    // 1a. Pre-fetch the context the model would otherwise call for. This
    // skips the cold-start exploration turns that Gemma 4 never recovers
    // from and Qwen 3.5 burns 3–5 rounds on.
    let hunks_json = mcp_call_json(&mut mcp, "floe.list_hunks", Value::Object(Default::default()))
        .await
        .context("pre-fetch list_hunks")?;
    let clusters_json =
        mcp_call_json(&mut mcp, "floe.list_flows_initial", Value::Object(Default::default()))
            .await
            .context("pre-fetch list_flows_initial")?;
    tracing::info!(
        hunks_bytes = hunks_json.len(),
        clusters_bytes = clusters_json.len(),
        "pre-injected discovery context"
    );

    // 2. Render the prompt.
    let rendered = prompt::render(PromptInputs {
        version: &cfg.prompt_version,
        hunk_count,
        initial_cluster_count,
        max_tool_calls: 200,
    })?;
    tracing::info!(version = %rendered.version, bytes = rendered.body.len(), "prompt rendered");

    // 3. Build the provider-specific client. Both return the same
    // ChatResponse shape so the loop below is provider-agnostic.
    enum Client {
        Ollama(OllamaClient),
        Glm(GlmClient),
    }
    impl Client {
        async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
            match self {
                Client::Ollama(c) => c.chat(req).await,
                Client::Glm(c) => c.chat(req).await,
            }
        }
    }
    let client = match cfg.provider {
        LlmProvider::Ollama => Client::Ollama(OllamaClient::new(&cfg.base_url)),
        LlmProvider::Glm => {
            let key = cfg
                .api_key
                .clone()
                .ok_or_else(|| anyhow!("glm provider requires FLOE_GLM_API_KEY"))?;
            Client::Glm(GlmClient::new(&cfg.base_url, key))
        }
    };

    // 4. Agent loop. The initial user message carries the pre-fetched
    // hunks + clusters so the model starts at synthesis, not discovery.
    let initial_user = format!(
        "Hunks ({hunks}): {hunks_json}\n\n\
         Initial structural clusters ({clusters}): {clusters_json}\n\n\
         Synthesize the flows. Emit floe.propose_flow / floe.remove_flow / \
         floe.finalize tool calls only — do not describe the plan in prose.",
        hunks = hunk_count,
        clusters = initial_cluster_count,
        hunks_json = hunks_json,
        clusters_json = clusters_json,
    );
    let mut messages: Vec<ChatMessage> = vec![
        ChatMessage {
            role: "system".into(),
            content: rendered.body,
            tool_calls: Vec::new(),
            tool_name: None,
        },
        ChatMessage {
            role: "user".into(),
            content: initial_user,
            tool_calls: Vec::new(),
            tool_name: None,
        },
    ];

    let mut consecutive_empty: u32 = 0;
    for turn in 0..MAX_TURNS {
        tracing::info!(turn, "ollama chat turn");
        let resp = client
            .chat(ChatRequest {
                model: cfg.model.clone(),
                messages: messages.clone(),
                tools: tools.clone(),
                stream: false,
                options: Some(json!({
                    "num_ctx": cfg.num_ctx,
                    "num_predict": cfg.num_predict,
                    "temperature": cfg.temperature,
                })),
                keep_alive: match cfg.provider {
                    LlmProvider::Ollama => Some(cfg.keep_alive.clone()),
                    LlmProvider::Glm => None,
                },
            })
            .await
            .with_context(|| format!("chat turn {turn}"))?;


        if resp.message.tool_calls.is_empty() {
            // GLM-4.7 sometimes leaks tool calls as inline
            // `<tool_call>…</tool_call>` XML in `content` instead of
            // the OpenAI function-calling API. Try to rehydrate
            // before treating the turn as "empty". Same recovery as
            // the intent pipeline — see `tool_call_drift` module.
            let rehydrated = parse_inline_glm_tool_calls(&resp.message.content);
            if !rehydrated.is_empty() {
                tracing::info!(
                    count = rehydrated.len(),
                    "rehydrated {} inline tool call(s) from content",
                    rehydrated.len()
                );
                messages.push(resp.message.clone());
                let mut saw_finalize = false;
                let mut finalize_outcome: Option<SynthesisOutcome> = None;
                for (name, args) in &rehydrated {
                    let synthetic = ToolCall {
                        function: super::ollama_client::ToolCallFunction {
                            name: name.clone(),
                            arguments: args.clone(),
                        },
                    };
                    let (reply, maybe_outcome) =
                        dispatch_tool_call(&mut mcp, &synthetic).await?;
                    messages.push(reply);
                    if let Some(o) = maybe_outcome {
                        saw_finalize = true;
                        finalize_outcome = Some(o);
                        break;
                    }
                }
                if saw_finalize {
                    let _ = mcp.shutdown().await;
                    return Ok(finalize_outcome.unwrap());
                }
                consecutive_empty = 0;
                continue;
            }

            consecutive_empty += 1;
            let has_content = !resp.message.content.trim().is_empty();
            tracing::warn!(
                turn,
                consecutive_empty,
                has_content,
                content = %truncate(&resp.message.content, 240),
                "model returned no tool_calls"
            );
            if consecutive_empty >= MAX_CONSECUTIVE_EMPTY {
                let _ = mcp.shutdown().await;
                return Ok(SynthesisOutcome::NoFinalize);
            }
            if has_content {
                // Model wrote a plan in prose instead of calling tools —
                // keep the plan in history and nudge it to execute.
                // Qwen 3.5 27B tends to do this once it thinks it has
                // "finished" the exploration phase.
                messages.push(resp.message.clone());
                messages.push(ChatMessage {
                    role: "user".into(),
                    content: "Good analysis. Now execute it: call floe.propose_flow for each flow in your plan above (name + rationale + hunk_ids), then floe.remove_flow on each structural cluster whose hunks are now covered, then floe.finalize. Do not respond with prose — only tool calls.".into(),
                    tool_calls: Vec::new(),
                    tool_name: None,
                });
            } else {
                // Truly empty. Don't append — appending an empty
                // assistant message shifts the KV-cache prefix and tends
                // to reinforce the stuck state.
            }
            continue;
        }
        consecutive_empty = 0;

        // Record the assistant turn so subsequent rounds keep context.
        messages.push(resp.message.clone());

        // Dispatch each requested tool call in order.
        let mut saw_finalize = false;
        let mut finalize_outcome: Option<SynthesisOutcome> = None;
        for tc in &resp.message.tool_calls {
            let (reply, maybe_outcome) = dispatch_tool_call(&mut mcp, tc).await?;
            messages.push(reply);
            if let Some(o) = maybe_outcome {
                saw_finalize = true;
                finalize_outcome = Some(o);
                break;
            }
        }
        if saw_finalize {
            let _ = mcp.shutdown().await;
            return Ok(finalize_outcome.unwrap());
        }
    }

    let _ = mcp.shutdown().await;
    Ok(SynthesisOutcome::NoFinalize)
}

/// Run one tool call through the MCP child. Returns the `role: "tool"`
/// message to feed back to Ollama, plus (for `floe.finalize`) the parsed
/// outcome that terminates the loop.
async fn dispatch_tool_call(
    mcp: &mut McpClient,
    tc: &ToolCall,
) -> Result<(ChatMessage, Option<SynthesisOutcome>)> {
    let name = &tc.function.name;
    // Defend against GLM drift: when `arguments` isn't an object
    // (stringified JSON, HTML-entity-encoded, `key=value` shell
    // style, single-element array) coerce into a proper object.
    // Object arguments pass through unchanged.
    let args = coerce_malformed_arguments(tc.function.arguments.clone());
    tracing::info!(tool = %name, args = %truncate(&args.to_string(), 200), "tool call");

    let result: ToolCallResult = mcp.call_tool(name, args).await?;

    // If this was the finalize call, parse the outcome from the tool's
    // text block. The server wraps the result as `{ outcome, flows | ... }`.
    let maybe_outcome = if name == "floe.finalize" && !result.is_error {
        Some(parse_finalize_outcome(&result.text)?)
    } else {
        None
    };

    let reply = ChatMessage {
        role: "tool".into(),
        content: result.text,
        tool_calls: Vec::new(),
        tool_name: Some(name.clone()),
    };
    Ok((reply, maybe_outcome))
}

fn parse_finalize_outcome(text: &str) -> Result<SynthesisOutcome> {
    let v: Value = serde_json::from_str(text)
        .with_context(|| format!("parsing finalize payload: {text}"))?;
    let outcome = v
        .get("outcome")
        .and_then(|x| x.as_str())
        .ok_or_else(|| anyhow!("finalize payload missing `outcome`"))?;
    match outcome {
        "accepted" => {
            let flows: Vec<Flow> = serde_json::from_value(
                v.get("flows").cloned().unwrap_or(Value::Array(vec![])),
            )
            .context("parsing accepted flows")?;
            Ok(SynthesisOutcome::Accepted(flows))
        }
        "rejected" => Ok(SynthesisOutcome::Rejected {
            rule: v
                .get("rejected_rule")
                .and_then(|x| x.as_str())
                .unwrap_or("unknown")
                .to_string(),
            detail: v
                .get("detail")
                .and_then(|x| x.as_str())
                .unwrap_or("")
                .to_string(),
        }),
        other => Err(anyhow!("unknown finalize outcome `{other}`")),
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Call an MCP tool from Rust (the host) rather than from the LLM. Used
/// for the pre-injection of hunks and structural clusters so the model
/// starts at synthesis instead of burning turns on discovery.
///
/// Returns the text content of the tool response verbatim — usually a
/// pretty-printed JSON array / object. Errors surface both transport
/// failures and tool-reported errors (isError=true).
async fn mcp_call_json(mcp: &mut McpClient, tool: &str, args: Value) -> Result<String> {
    let r = mcp.call_tool(tool, args).await?;
    if r.is_error {
        return Err(anyhow!("pre-fetch {tool} returned isError: {}", r.text));
    }
    Ok(r.text)
}
