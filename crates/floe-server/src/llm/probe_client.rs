//! Probe-side adapter. Wires `floe-probe`'s [`ProbeClient`] trait to the
//! existing Ollama / GLM chat clients plus an `floe-mcp` child process
//! for tool dispatch.
//!
//! Responsibilities:
//!
//! - **Chat**: forward to the wrapped [`Client`] (Ollama or GLM), map
//!   its [`ChatResponse`] onto [`floe_probe::ChatReply`].
//! - **Tool list**: the MCP child's `tools/list`, pared down to the
//!   probe-safe subset (`list_entities` / `list_hunks` / `get_entity` /
//!   `neighbors`). Mutation tools are intentionally NOT exposed — the
//!   probe reads, it doesn't synthesise.
//! - **Tool dispatch**: forward to the MCP child, parse the result, and
//!   extract every qualified name the call surfaced so the probe
//!   session can tally per-entity visits.

use std::sync::Arc;

use floe_probe::{ChatReply, Msg, ProbeClient, ToolDef, ToolDispatchResult};
use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use tokio::sync::Mutex;

use super::glm_client::GlmClient;
use super::mcp_client::McpClient;
use super::ollama_client::{
    ChatMessage, ChatRequest, OllamaClient, ToolCall, ToolCallFunction, ToolDef as OllamaToolDef,
    ToolDefFunction as OllamaToolDefFunction,
};

/// The probe-safe subset of MCP tools. Mutation tools are filtered out
/// at tools/list time so the model can't propose flows from inside a
/// probe session by accident.
const PROBE_SAFE_TOOLS: &[&str] = &[
    "floe.list_entities",
    "floe.list_hunks",
    "floe.get_entity",
    "floe.neighbors",
];

pub enum BackingClient {
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

pub struct McpProbeClient {
    backing: BackingClient,
    mcp: Arc<Mutex<McpClient>>,
    tools: Vec<ToolDef>,
    model: String,
    keep_alive: Option<String>,
    options: Option<Value>,
}

impl McpProbeClient {
    /// Build the client. `tool_specs` is the full MCP tools/list output —
    /// we filter it down to the probe-safe subset here.
    pub fn new(
        backing: BackingClient,
        mcp: Arc<Mutex<McpClient>>,
        tool_specs: Vec<super::mcp_client::ToolSpec>,
        model: String,
        keep_alive: Option<String>,
        options: Option<Value>,
    ) -> Self {
        let tools: Vec<ToolDef> = tool_specs
            .into_iter()
            .filter(|t| PROBE_SAFE_TOOLS.contains(&t.name.as_str()))
            .map(|spec| ToolDef {
                kind: "function",
                function: floe_probe::session::ToolDefFunction {
                    name: spec.name,
                    description: spec.description,
                    parameters: spec.input_schema,
                },
            })
            .collect();
        Self {
            backing,
            mcp,
            tools,
            model,
            keep_alive,
            options,
        }
    }
}

impl ProbeClient for McpProbeClient {
    async fn chat(
        &self,
        messages: Vec<Msg>,
        tools: Vec<ToolDef>,
    ) -> Result<ChatReply> {
        // Convert `floe-probe`'s Msg/ToolDef into our ollama-shape types.
        let req = ChatRequest {
            model: self.model.clone(),
            messages: messages.into_iter().map(probe_to_ollama_msg).collect(),
            tools: tools.into_iter().map(probe_to_ollama_tool).collect(),
            stream: false,
            options: self.options.clone(),
            keep_alive: self.keep_alive.clone(),
        };
        let resp = self.backing.chat(req).await?;
        Ok(ChatReply {
            message: Msg {
                role: resp.message.role,
                content: resp.message.content,
                tool_calls: resp
                    .message
                    .tool_calls
                    .into_iter()
                    .map(|tc| floe_probe::session::ToolCall {
                        function: floe_probe::session::ToolCallFunction {
                            name: tc.function.name,
                            arguments: tc.function.arguments,
                        },
                    })
                    .collect(),
                tool_name: resp.message.tool_name,
            },
            tokens_in: resp.tokens_in,
            tokens_out: resp.tokens_out,
            done: resp.done,
        })
    }

    async fn dispatch_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> Result<ToolDispatchResult> {
        // Allow only probe-safe tools through; anything else is a bug.
        if !PROBE_SAFE_TOOLS.contains(&name) {
            return Err(anyhow!(
                "probe tried to call non-probe-safe tool `{name}` — this is a programmer error"
            ));
        }
        let result = self.mcp.lock().await.call_tool(name, arguments.clone()).await?;
        let entities = extract_entities(name, &arguments, &result.text);
        Ok(ToolDispatchResult {
            text: result.text,
            is_error: result.is_error,
            entities_touched: entities,
        })
    }

    fn tools(&self) -> Vec<ToolDef> {
        self.tools.clone()
    }
}

/* -------------------------------------------------------------------------- */
/* Message / tool-def shape conversion                                        */
/* -------------------------------------------------------------------------- */

fn probe_to_ollama_msg(m: Msg) -> ChatMessage {
    ChatMessage {
        role: m.role,
        content: m.content,
        tool_calls: m
            .tool_calls
            .into_iter()
            .map(|tc| ToolCall {
                function: ToolCallFunction {
                    name: tc.function.name,
                    arguments: tc.function.arguments,
                },
            })
            .collect(),
        tool_name: m.tool_name,
    }
}

fn probe_to_ollama_tool(t: ToolDef) -> OllamaToolDef {
    OllamaToolDef {
        kind: "function",
        function: OllamaToolDefFunction {
            name: t.function.name,
            description: t.function.description,
            parameters: t.function.parameters,
        },
    }
}

/* -------------------------------------------------------------------------- */
/* Entity extraction                                                          */
/* -------------------------------------------------------------------------- */

/// Pull qualified names out of a tool call + its result. What we can
/// lift depends on the tool:
///
/// - `floe.get_entity(id)` → the `id` argument is a qualified name.
/// - `floe.neighbors(id, …)` → the arg's `id` plus every `name` field in
///   the returned `nodes[]` array.
/// - `adr.list_entities(…)` → every `name` in the returned array.
/// - `floe.list_hunks()` → every entry in each hunk's `entities[]`.
///
/// The result text is pretty-printed JSON (that's what the MCP server
/// wraps tool results in); we parse it best-effort, swallowing parse
/// errors because a malformed result just means "no entities extracted
/// for this call" — not a showstopper.
fn extract_entities(name: &str, arguments: &Value, result_text: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    match name {
        "floe.get_entity" => {
            if let Some(id) = arguments.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_string());
            }
        }
        "floe.neighbors" => {
            if let Some(id) = arguments.get("id").and_then(|v| v.as_str()) {
                out.push(id.to_string());
            }
            if let Ok(parsed) = serde_json::from_str::<Value>(result_text) {
                if let Some(nodes) = parsed.get("nodes").and_then(|v| v.as_array()) {
                    for n in nodes {
                        if let Some(name) = n.get("name").and_then(|v| v.as_str()) {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        "floe.list_entities" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(result_text) {
                if let Some(arr) = parsed.as_array() {
                    for e in arr {
                        if let Some(name) = e.get("name").and_then(|v| v.as_str()) {
                            out.push(name.to_string());
                        }
                    }
                }
            }
        }
        "floe.list_hunks" => {
            if let Ok(parsed) = serde_json::from_str::<Value>(result_text) {
                if let Some(arr) = parsed.as_array() {
                    for h in arr {
                        if let Some(ents) = h.get("entities").and_then(|v| v.as_array()) {
                            for e in ents {
                                if let Some(s) = e.as_str() {
                                    out.push(s.to_string());
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Helper the worker calls to pre-flight a probe run: spawns an MCP
/// child against the provided artifact JSON path, initializes the
/// handshake, and returns the child wrapped in a `Mutex` the probe
/// client can share across sessions.
pub async fn spawn_probe_mcp(artifact_path: &std::path::Path) -> Result<(Arc<Mutex<McpClient>>, Vec<super::mcp_client::ToolSpec>)> {
    let mut mcp = McpClient::spawn_probe(
        artifact_path,
        "floe-probe",
        env!("CARGO_PKG_VERSION"),
    )
    .await
    .context("spawn floe-mcp for probe")?;
    mcp.initialize().await.context("mcp initialize for probe")?;
    let tools = mcp.list_tools().await.context("mcp tools/list for probe")?;
    Ok((Arc::new(Mutex::new(mcp)), tools))
}
