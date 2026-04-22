//! Minimal Ollama chat client — just the bits we need for the flow
//! synthesis loop. Hits `POST /api/chat` with non-streaming responses and
//! tool-call support.
//!
//! Schema reference: <https://ollama.com/blog/tool-support>. Tools use the
//! OpenAI-compat shape (`type: "function"`, `function: { name, description,
//! parameters }`). Assistant responses surface tool calls at
//! `message.tool_calls[]`, with `function.arguments` as a JSON **object**
//! (not a stringified payload — that differs from OpenAI).

use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub struct OllamaClient {
    base_url: String,
    http: reqwest::Client,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChatRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<ToolDef>,
    pub stream: bool,
    /// Passed through to Ollama's options block. We set a generous
    /// num_predict so multi-turn loops don't cut off mid tool call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub options: Option<Value>,
    /// How long Ollama keeps the model loaded after this request (e.g.
    /// `"10m"`). Ollama parses durations server-side; passing this on
    /// every turn keeps the model resident across the whole synthesis.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub keep_alive: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
    /// Name of the tool the `role: "tool"` message is answering for.
    /// Some Ollama builds want this; others ignore it. Harmless to include.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub function: ToolCallFunction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallFunction {
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDef {
    #[serde(rename = "type")]
    pub kind: &'static str,
    pub function: ToolDefFunction,
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolDefFunction {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ChatResponse {
    pub message: ChatMessage,
    #[serde(default)]
    pub done: bool,
    #[serde(default)]
    pub done_reason: Option<String>,
    /// Ollama reports prompt + generated token counts; GLM reports
    /// `usage.prompt_tokens` + `usage.completion_tokens`. Both clients
    /// normalise into these fields so the probe pass can measure effort
    /// without caring which provider answered.
    #[serde(default, rename = "prompt_eval_count")]
    pub tokens_in: u32,
    #[serde(default, rename = "eval_count")]
    pub tokens_out: u32,
}

impl OllamaClient {
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::builder()
                // Gemma 4 26B first-token latency can hit ~30s on cold
                // load; cap the whole chat at 10 min to accommodate
                // long-ish multi-step tool reasoning.
                .timeout(std::time::Duration::from_secs(600))
                .build()
                .expect("reqwest client"),
        }
    }

    pub async fn chat(&self, req: ChatRequest) -> Result<ChatResponse> {
        let url = format!("{}/api/chat", self.base_url.trim_end_matches('/'));
        let resp = self
            .http
            .post(&url)
            .json(&req)
            .send()
            .await
            .with_context(|| format!("POST {url}"))?;
        let status = resp.status();
        let body_text = resp.text().await.context("reading ollama body")?;
        if !status.is_success() {
            return Err(anyhow!("ollama HTTP {status}: {body_text}"));
        }
        serde_json::from_str(&body_text)
            .with_context(|| format!("parsing ollama response: {body_text}"))
    }

    /// Build a [`ToolDef`] from an MCP tool spec — the JSON shape matches
    /// one-to-one after a rename from `inputSchema` to `parameters`.
    pub fn tool_def_from_mcp(spec: &super::mcp_client::ToolSpec) -> ToolDef {
        ToolDef {
            kind: "function",
            function: ToolDefFunction {
                name: spec.name.clone(),
                description: spec.description.clone(),
                parameters: spec.input_schema.clone(),
            },
        }
    }
}
