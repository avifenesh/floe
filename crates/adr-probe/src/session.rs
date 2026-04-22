//! A single probe session: runs one [`ProbeDef`] against a
//! caller-supplied [`ProbeClient`], records per-entity observations, and
//! returns a [`ProbeResult`].
//!
//! The crate owns no LLM transport. `adr-server` implements
//! [`ProbeClient`] using whichever chat client matches the pinned probe
//! model (ollama or GLM).

use std::collections::HashMap;
use std::future::Future;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::probes::{ProbeDef, ProbeId};

/// One chat message as the probe loop sees it. Mirrors the OpenAI-shape
/// used by our Ollama + GLM clients — kept in this crate so downstream
/// doesn't need a server dependency.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Msg {
    pub role: String,
    #[serde(default)]
    pub content: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCall>,
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

/// What the caller's chat returns. Matches our existing
/// `ollama_client::ChatResponse` on the server side.
pub struct ChatReply {
    pub message: Msg,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub done: bool,
}

/// Trait the caller implements to let the session call the LLM +
/// dispatch tool calls. The session doesn't know about Ollama vs GLM,
/// stdio MCP, etc. — all of that lives in the caller's adapter.
pub trait ProbeClient {
    /// Send a chat turn. Must honour `tools` via tool-calling.
    fn chat(
        &self,
        messages: Vec<Msg>,
        tools: Vec<ToolDef>,
    ) -> impl Future<Output = Result<ChatReply>> + Send;

    /// Dispatch one tool call the model emitted. Return the text payload
    /// the model will see as `role: "tool"` content.
    fn dispatch_tool(
        &self,
        name: &str,
        arguments: Value,
    ) -> impl Future<Output = Result<ToolDispatchResult>> + Send;

    /// List of tool defs to pass on every chat turn. Stable for the
    /// lifetime of the session.
    fn tools(&self) -> Vec<ToolDef>;
}

#[derive(Debug, Clone)]
pub struct ToolDispatchResult {
    pub text: String,
    pub is_error: bool,
    /// Qualified names of entities this tool call surfaced. Used to
    /// tally per-entity visit counts. Callers extract whatever they can
    /// from the tool's result; empty is acceptable.
    pub entities_touched: Vec<String>,
}

/// Aggregated observations from a single probe session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResult {
    pub probe_id: ProbeId,
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub duration_ms: u64,
    /// Per-entity visit counts from all tool calls during the session.
    pub per_entity_visits: HashMap<String, u32>,
    /// Final assistant message content for debugging. Not used in cost
    /// computation — the measurement *is* the effort, not the answer.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub final_answer: String,
    /// Reason the session ended: `"completed"`, `"max-turns"`, `"errored"`.
    pub end_reason: String,
}

pub struct ProbeSession<'a> {
    def: &'a ProbeDef,
}

impl<'a> ProbeSession<'a> {
    pub fn new(def: &'a ProbeDef) -> Self {
        Self { def }
    }

    /// Run the probe to completion or the turn cap. The session is
    /// "clean" — it constructs a fresh message history from the probe's
    /// system prompt + question and does not carry any state between
    /// probes.
    pub async fn run<C: ProbeClient>(&self, client: &C) -> Result<ProbeResult> {
        let start = std::time::Instant::now();
        let tools = client.tools();
        let mut messages: Vec<Msg> = vec![
            Msg {
                role: "system".into(),
                content: self.def.system_prompt.into(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
            Msg {
                role: "user".into(),
                content: self.def.question.into(),
                tool_calls: Vec::new(),
                tool_name: None,
            },
        ];

        let mut total_tokens_in: u32 = 0;
        let mut total_tokens_out: u32 = 0;
        let mut tool_calls_total: u32 = 0;
        let mut visits: HashMap<String, u32> = HashMap::new();
        let mut end_reason = "completed";
        let mut final_answer = String::new();
        let mut turns: u32 = 0;

        for turn in 0..self.def.max_turns {
            turns = turn + 1;
            let reply = client.chat(messages.clone(), tools.clone()).await?;
            total_tokens_in += reply.tokens_in;
            total_tokens_out += reply.tokens_out;

            let tool_calls = reply.message.tool_calls.clone();
            messages.push(reply.message.clone());

            if tool_calls.is_empty() {
                // Model ended with a text answer; record and stop.
                final_answer = reply.message.content;
                break;
            }

            for tc in &tool_calls {
                tool_calls_total += 1;
                match client
                    .dispatch_tool(&tc.function.name, tc.function.arguments.clone())
                    .await
                {
                    Ok(result) => {
                        for e in &result.entities_touched {
                            *visits.entry(e.clone()).or_default() += 1;
                        }
                        messages.push(Msg {
                            role: "tool".into(),
                            content: result.text,
                            tool_calls: Vec::new(),
                            tool_name: Some(tc.function.name.clone()),
                        });
                    }
                    Err(e) => {
                        messages.push(Msg {
                            role: "tool".into(),
                            content: format!("ERROR: tool dispatch failed: {e}"),
                            tool_calls: Vec::new(),
                            tool_name: Some(tc.function.name.clone()),
                        });
                    }
                }
            }
        }

        if turns >= self.def.max_turns {
            end_reason = "max-turns";
        }

        Ok(ProbeResult {
            probe_id: self.def.id,
            turns,
            tool_calls: tool_calls_total,
            tokens_in: total_tokens_in,
            tokens_out: total_tokens_out,
            duration_ms: start.elapsed().as_millis() as u64,
            per_entity_visits: visits,
            final_answer,
            end_reason: end_reason.into(),
        })
    }
}
