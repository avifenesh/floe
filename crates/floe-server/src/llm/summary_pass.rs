//! PR summary pass — one short GLM call that produces a
//! reviewer-facing headline and optional 1–2 sentence description
//! for the PR as a whole. The output replaces the raw `repo#N`
//! identifier in the top spine / sidebar.
//!
//! Why its own pass (rather than reusing intent_pipeline):
//! - Runs on every analysis — even without intent — so we can't gate
//!   it behind an intent-supplied flag.
//! - Describes the *code change*, not the stated intent. The intent
//!   summary (already produced) is an upstream signal; this synthesises
//!   across intent + actual structural delta.
//! - Uses GLM-4.6 by default (cheaper/faster than 4.7), per user
//!   directive — summary is a short one-shot pass and doesn't need the
//!   heavier proof budget.

use anyhow::{anyhow, Context, Result};
use serde::Deserialize;
use serde_json::json;

use floe_core::{Artifact, IntentInput, PrSummary};

use super::config::{LlmConfig, LlmProvider};
use super::glm_client::GlmClient;
use super::ollama_client::{ChatMessage, ChatRequest, OllamaClient};

const SYSTEM: &str = "You summarise GitHub pull requests for a reviewer. \
You are given the PR's stated intent (if any) and a compact description \
of its architectural delta (entities touched, flow names, hunk counts). \
Produce a short reviewer-facing title and an optional 1–2 sentence \
description. The title should describe the change in plain English, not \
quote commit messages. Omit the description when the title is already \
self-explanatory. \
\
Respond with JSON ONLY of the form: \
{\"headline\":\"<≤60 chars>\",\"description\":\"<optional, ≤280 chars, or null>\"}";

#[derive(Debug, Deserialize)]
struct RawSummary {
    headline: String,
    #[serde(default)]
    description: Option<String>,
}

/// Render the compact structural-delta context the prompt needs.
fn render_context(artifact: &Artifact) -> String {
    let mut out = String::new();
    // Intent preview.
    if let Some(i) = &artifact.intent {
        match i {
            IntentInput::Structured(intent) => {
                out.push_str(&format!("Stated intent: {}\n", intent.title));
                if !intent.summary.is_empty() {
                    out.push_str(&format!("  {}\n", intent.summary));
                }
                for (i, c) in intent.claims.iter().take(3).enumerate() {
                    out.push_str(&format!("  claim {}: {}\n", i + 1, c.statement));
                }
            }
            IntentInput::RawText(t) => {
                let trimmed = t.trim();
                let preview = if trimmed.len() > 500 {
                    format!("{}…", &trimmed[..500])
                } else {
                    trimmed.to_string()
                };
                out.push_str(&format!("Stated intent (raw): {preview}\n"));
            }
        }
    } else {
        out.push_str("Stated intent: (none supplied)\n");
    }

    // Hunk totals by kind.
    let mut calls = 0u32;
    let mut states = 0u32;
    let mut apis = 0u32;
    let mut locks = 0u32;
    let mut datas = 0u32;
    let mut docs = 0u32;
    let mut dels = 0u32;
    for h in &artifact.hunks {
        use floe_core::HunkKind::*;
        match &h.kind {
            Call { .. } => calls += 1,
            State { .. } => states += 1,
            Api { .. } => apis += 1,
            Lock { .. } => locks += 1,
            Data { .. } => datas += 1,
            Docs { .. } => docs += 1,
            Deletion { .. } => dels += 1,
        }
    }
    out.push_str(&format!(
        "Delta: {} call / {} state / {} api / {} lock / {} data / {} docs / {} del hunk(s)\n",
        calls, states, apis, locks, datas, docs, dels
    ));

    // Top flow names + entity set (up to 5 flows, up to 4 entities each).
    let flows = &artifact.flows;
    out.push_str(&format!("Flows ({}):\n", flows.len()));
    for f in flows.iter().take(5) {
        let ents = f
            .entities
            .iter()
            .take(4)
            .cloned()
            .collect::<Vec<_>>()
            .join(", ");
        out.push_str(&format!("  - {}  [entities: {}]\n", f.name, ents));
    }
    out
}

pub async fn run_summary(cfg: &LlmConfig, artifact: &Artifact) -> Result<PrSummary> {
    let user = render_context(artifact);
    let req = ChatRequest {
        model: cfg.model.clone(),
        messages: vec![
            ChatMessage {
                role: "system".into(),
                content: SYSTEM.into(),
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
        // GLM-4.6 spends a chunk of every response on reasoning
        // content; at 280 tokens it routinely hits finish_reason=length
        // before emitting the JSON. Bump to 1200 and disable the
        // thinking track when the provider honours it. `do_sample`
        // keeps temperature applied; `thinking`/`enable_thinking` is
        // the GLM knob — ignored by models that don't recognise it.
        options: Some(json!({
            "temperature": 0.3,
            "num_predict": 1200,
            "max_tokens": 1200,
            "thinking": { "type": "disabled" },
            "enable_thinking": false
        })),
        keep_alive: None,
    };
    let content = match cfg.provider {
        LlmProvider::Glm => {
            let key = cfg
                .api_key
                .clone()
                .ok_or_else(|| anyhow!("FLOE_GLM_API_KEY required for summary pass"))?;
            let client = GlmClient::new(cfg.base_url.clone(), key);
            client
                .chat(req)
                .await
                .context("glm summary pass")?
                .message
                .content
        }
        LlmProvider::Ollama => {
            let client = OllamaClient::new(cfg.base_url.clone());
            client
                .chat(req)
                .await
                .context("ollama summary pass")?
                .message
                .content
        }
    };
    let body = extract_json_object(content.trim())
        .ok_or_else(|| anyhow!("summary response had no JSON object: {content}"))?;
    let raw: RawSummary = serde_json::from_str(body)
        .with_context(|| format!("summary parse: {body}"))?;
    let headline = raw.headline.trim();
    if headline.is_empty() {
        return Err(anyhow!("summary returned empty headline"));
    }
    let description = raw
        .description
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && s != "null");
    Ok(PrSummary {
        headline: truncate(headline, 80),
        description: description.map(|s| truncate(&s, 320)),
        model: cfg.model.clone(),
    })
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    out.push('…');
    out
}

/// Grab the first top-level JSON object substring in `s`. Returns
/// `None` when no `{ … }` block is present. Tolerates GLM's habit of
/// wrapping JSON in ``` fences or prose preamble.
fn extract_json_object(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escape = false;
    let bytes = s.as_bytes();
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escape {
                escape = false;
            } else if c == '\\' {
                escape = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_object_from_prose() {
        let s = "here is the result:\n```\n{\"headline\":\"x\",\"description\":null}\n```";
        let js = extract_json_object(s).unwrap();
        assert!(js.contains("headline"));
    }

    #[test]
    fn truncate_respects_char_boundary() {
        let s = "éééééééé";
        assert_eq!(truncate(s, 3), "éé…");
    }

    #[test]
    fn truncate_short_returns_unchanged() {
        assert_eq!(truncate("hi", 5), "hi");
    }
}
