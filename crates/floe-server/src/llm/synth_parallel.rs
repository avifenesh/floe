//! Per-cluster parallel flow synthesis.
//!
//! The classic synth loop (`synthesize.rs`) gives the model the full
//! hunk list + every structural cluster and asks it to propose,
//! merge, and finalise via MCP tool calls over 5–40 turns. Two
//! pathologies we saw on glide-mq #181:
//!
//! 1. GLM-4.7 drifts into its native XML tool-call template mid-loop
//!    → `arguments` becomes stringified/escaped/positional-JSON
//!    → the host parser reads `{}`, rejects, the model retries the
//!    same broken shape for 40 turns, falls back to structural.
//! 2. Even when it doesn't drift, serial turns at 5–12s each add up:
//!    5 clusters × 6 turns × 8s ≈ 4 minutes of wall time.
//!
//! This module takes a different shape:
//!
//! - **No MCP.** No tool calls, no multi-turn agent loop. One chat
//!   request per cluster, emitting one JSON object. The model can't
//!   drift what it can't reach.
//! - **Parallel.** N clusters = N concurrent GLM calls, bounded by
//!   the process-wide `FLOE_GLM_CONCURRENCY` semaphore + circuit
//!   breaker already in place in [`super::glm_client`]. Total latency
//!   collapses to ≈ max(per-cluster).
//! - **Deterministic merge.** We assemble the final `Vec<Flow>` from
//!   the per-cluster JSON results — no LLM is trusted to validate
//!   hunk coverage. A cluster that fails to produce valid JSON falls
//!   back to its structural placeholder; other clusters still win.
//!
//! The prompt is intentionally tiny — one system message defining the
//! output shape, one user message with the cluster. No worked
//! example, no budget language, no "use the function API" nudges
//! (none apply — we're not using tools).

use std::sync::Arc;

use floe_core::{Artifact, Flow, FlowSource, HunkKind};
use anyhow::{anyhow, Context, Result};
use futures::future::join_all;
use serde::Deserialize;
use serde_json::{json, Value};

use super::config::{LlmConfig, LlmProvider};
use super::glm_client::GlmClient;
use super::ollama_client::{ChatMessage, ChatRequest, OllamaClient};
use super::synthesize::SynthesisOutcome;

/// Hard cap on how long a single cluster session may cost. The
/// semaphore + breaker in `glm_client` bound burst + rate; this is
/// the per-call ceiling the model itself sees. Well under the 10 min
/// reqwest connection timeout.
const PER_CLUSTER_TIMEOUT_SECS: u64 = 120;

/// Reserved flow names — same list the MCP validator enforces. We
/// re-apply it here so cluster proposals that hit a reserved word
/// fall back to the structural placeholder instead of silently
/// shipping a boring name.
const RESERVED_NAMES: &[&str] = &["misc", "various", "other", "unknown", "cluster", "group"];

const SYSTEM_PROMPT: &str = "\
You name one flow of a pull request. You receive:

- A flow's current structural placeholder name (e.g. `<structural: Queue>`) and rationale.
- The list of hunks in that flow (each has an id, a kind, and the qualified entities it touches).
- The list of entities the flow covers.

Your one task: replace the placeholder with a real name and rationale that describe the architectural story.

Emit EXACTLY one JSON object, nothing else — no prose, no code fences, no XML. Shape:

{
  \"name\": \"<3-6 words, Title Case, no reserved words>\",
  \"rationale\": \"<one sentence; what this flow delivers, grounded in the hunks>\",
  \"split\": false
}

If the hunks in this cluster represent two or more distinct flows that were wrongly grouped, set \"split\" to an array of sub-flows:

{
  \"name\": \"<fallback name or the dominant flow>\",
  \"rationale\": \"<one sentence>\",
  \"split\": [
    { \"name\": \"<subflow name>\", \"rationale\": \"<one sentence>\", \"hunk_ids\": [\"<ids from input>\"] }
  ]
}

When \"split\" is an array, every hunk in the input must appear in exactly one sub-flow. Keep splits rare — prefer a single well-named flow.

Reserved words you must never use: misc, various, other, unknown, cluster, group.
";

/// Run per-cluster synth in parallel. `cfg` is the same LLM config
/// the classic synth used (`FLOE_LLM`). Returns a [`SynthesisOutcome`]
/// so callers can swap between the two paths transparently.
pub async fn synthesize_parallel(artifact: &Artifact, cfg: &LlmConfig) -> SynthesisOutcome {
    match run(artifact, cfg).await {
        Ok(flows) => SynthesisOutcome::Accepted(flows),
        Err(e) => {
            tracing::warn!(error = %format!("{e:#}"), "parallel synth errored");
            SynthesisOutcome::Errored(format!("{e:#}"))
        }
    }
}

async fn run(artifact: &Artifact, cfg: &LlmConfig) -> Result<Vec<Flow>> {
    let clusters = artifact.flows.clone();
    if clusters.is_empty() {
        return Ok(clusters);
    }
    tracing::info!(
        clusters = clusters.len(),
        model = %cfg.model,
        "parallel synth starting"
    );

    let cfg = Arc::new(cfg.clone());
    let artifact = Arc::new(artifact.clone());

    // Fan out — each cluster gets one async future.
    let futures = clusters.iter().enumerate().map(|(idx, flow)| {
        let cfg = Arc::clone(&cfg);
        let artifact = Arc::clone(&artifact);
        let cluster = flow.clone();
        async move {
            let outcome = tokio::time::timeout(
                std::time::Duration::from_secs(PER_CLUSTER_TIMEOUT_SECS),
                synth_one_cluster(&artifact, &cluster, &cfg),
            )
            .await;
            match outcome {
                Ok(Ok(proposal)) => {
                    tracing::info!(
                        idx,
                        flow_id = %cluster.id,
                        name = %proposal.name,
                        split = proposal.split.is_some(),
                        "cluster synth ok"
                    );
                    Ok::<_, anyhow::Error>((cluster, Some(proposal)))
                }
                Ok(Err(e)) => {
                    tracing::warn!(
                        idx,
                        flow_id = %cluster.id,
                        error = %format!("{e:#}"),
                        "cluster synth failed — keeping structural placeholder"
                    );
                    Ok((cluster, None))
                }
                Err(_) => {
                    tracing::warn!(
                        idx,
                        flow_id = %cluster.id,
                        "cluster synth timed out — keeping structural placeholder"
                    );
                    Ok((cluster, None))
                }
            }
        }
    });
    let results = join_all(futures).await;

    // Assemble final Vec<Flow>. Valid proposals win; failed/timed-out
    // clusters keep their structural placeholder so coverage stays
    // intact (every hunk still belongs to at least one flow).
    let mut out: Vec<Flow> = Vec::new();
    let mut any_llm = false;
    let llm_stamp = FlowSource::Llm {
        model: cfg.model.clone(),
        version: cfg.prompt_version.clone(),
    };
    let mut order: u32 = 0;
    for result in results {
        let (cluster, proposal) = result?;
        match proposal {
            Some(p) if proposal_is_valid(&p, &cluster) => {
                any_llm = true;
                if let Some(subflows) = p.split.as_ref() {
                    // Each subflow becomes its own Flow. Preserve the
                    // cluster's entity set (deterministic — we don't
                    // ask the model to partition entities).
                    for sub in subflows {
                        let hunk_ids = sub.hunk_ids.clone();
                        let entities: Vec<String> = cluster
                            .entities
                            .iter()
                            .filter(|e| {
                                hunk_ids.iter().any(|hid| {
                                    let h = artifact.hunks.iter().find(|h| &h.id == hid);
                                    h.map(|h| hunk_touches(h, e)).unwrap_or(false)
                                })
                            })
                            .cloned()
                            .collect();
                        out.push(Flow {
                            id: format!("flow-{}", &sub.hunk_ids.join("|"))
                                .chars()
                                .take(72)
                                .collect(),
                            name: sub.name.clone(),
                            rationale: sub.rationale.clone(),
                            source: llm_stamp.clone(),
                            hunk_ids,
                            entities,
                            extra_entities: Vec::new(),
            propagation_edges: Vec::new(),
                            order,
                            evidence: Vec::new(),
                            cost: None,
                            intent_fit: None,
                            membership: None,
                            proof: None,
                        });
                        order += 1;
                    }
                } else {
                    out.push(Flow {
                        name: p.name.clone(),
                        rationale: p.rationale.clone(),
                        source: llm_stamp.clone(),
                        order,
                        ..cluster
                    });
                    order += 1;
                }
            }
            _ => {
                out.push(Flow { order, ..cluster });
                order += 1;
            }
        }
    }

    if !any_llm {
        // Nothing came back valid — don't advertise this as an LLM
        // run. The caller will treat it as "no synthesis".
        return Err(anyhow!(
            "no cluster returned a valid proposal (check model config + quota)"
        ));
    }

    Ok(out)
}

/// Run one GLM call for one cluster. Returns the parsed proposal or
/// an error. Does not retry — the per-call retry + circuit breaker
/// in [`super::glm_client`] already handle transient faults.
async fn synth_one_cluster(
    artifact: &Artifact,
    cluster: &Flow,
    cfg: &LlmConfig,
) -> Result<ClusterProposal> {
    let user_message = render_cluster(artifact, cluster);

    let messages = vec![
        ChatMessage {
            role: "system".into(),
            content: SYSTEM_PROMPT.into(),
            tool_calls: Vec::new(),
            tool_name: None,
        },
        ChatMessage {
            role: "user".into(),
            content: user_message,
            tool_calls: Vec::new(),
            tool_name: None,
        },
    ];

    let req = ChatRequest {
        model: cfg.model.clone(),
        messages,
        tools: Vec::new(),
        stream: false,
        options: Some(json!({
            "temperature": cfg.temperature,
            "num_predict": cfg.num_predict,
        })),
        keep_alive: match cfg.provider {
            LlmProvider::Ollama => Some(cfg.keep_alive.clone()),
            LlmProvider::Glm => None,
        },
    };

    let resp = match cfg.provider {
        LlmProvider::Ollama => {
            OllamaClient::new(cfg.base_url.clone())
                .chat(req)
                .await
                .context("ollama chat")?
        }
        LlmProvider::Glm => {
            let api_key = cfg
                .api_key
                .clone()
                .ok_or_else(|| anyhow!("glm without api key — check FLOE_GLM_API_KEY"))?;
            GlmClient::new(cfg.base_url.clone(), api_key)
                .chat(req)
                .await
                .context("glm chat")?
        }
    };

    let content = resp.message.content.trim();
    if content.is_empty() {
        return Err(anyhow!("empty response"));
    }
    extract_proposal(content)
}

/// Pull a JSON object out of the assistant's content and parse it
/// into a `ClusterProposal`. Tolerates prose-around-JSON and
/// code-fenced JSON.
fn extract_proposal(content: &str) -> Result<ClusterProposal> {
    // Fast path.
    if let Ok(p) = serde_json::from_str::<ClusterProposal>(content) {
        return Ok(p);
    }
    // Strip markdown code fences.
    let stripped = content
        .trim()
        .trim_start_matches("```json")
        .trim_start_matches("```")
        .trim_end_matches("```")
        .trim();
    if let Ok(p) = serde_json::from_str::<ClusterProposal>(stripped) {
        return Ok(p);
    }
    // Balanced-brace extract as last resort.
    if let Some(v) = first_json_object(content) {
        if let Ok(p) = serde_json::from_value::<ClusterProposal>(v) {
            return Ok(p);
        }
    }
    Err(anyhow!("no parseable JSON in content: {content}"))
}

fn first_json_object(s: &str) -> Option<Value> {
    let bytes = s.as_bytes();
    let mut start = None;
    let mut depth = 0usize;
    let mut in_str = false;
    let mut esc = false;
    for (i, &b) in bytes.iter().enumerate() {
        let c = b as char;
        if esc {
            esc = false;
            continue;
        }
        if in_str {
            if c == '\\' {
                esc = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
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
                    return serde_json::from_str(&s[from..=i]).ok();
                }
            }
            _ => {}
        }
    }
    None
}

/// Render one cluster into the user message.
fn render_cluster(artifact: &Artifact, flow: &Flow) -> String {
    let mut out = String::new();
    out.push_str("cluster:\n");
    out.push_str(&format!("  placeholder_name: {}\n", flow.name));
    out.push_str(&format!("  placeholder_rationale: {}\n", flow.rationale));
    out.push_str(&format!("  entities: {}\n", flow.entities.join(", ")));
    out.push_str("  hunks:\n");
    for hid in &flow.hunk_ids {
        let Some(h) = artifact.hunks.iter().find(|h| &h.id == hid) else {
            out.push_str(&format!("    - {hid} (missing)\n"));
            continue;
        };
        let (kind, extras) = match &h.kind {
            HunkKind::Call { added_edges, removed_edges } => (
                "call",
                format!(
                    "+{} edges, -{} edges",
                    added_edges.len(),
                    removed_edges.len()
                ),
            ),
            HunkKind::State { added_variants, removed_variants, .. } => (
                "state",
                format!(
                    "+{}, -{} variants",
                    added_variants.len(),
                    removed_variants.len()
                ),
            ),
            HunkKind::Api { before_signature, after_signature, .. } => {
                let before = before_signature.as_deref().unwrap_or("(new)");
                let after = after_signature.as_deref().unwrap_or("(removed)");
                ("api", format!("{before} → {after}"))
            }
            HunkKind::Lock { file, primitive, before, after } => {
                let b = before.as_deref().unwrap_or("(none)");
                let a = after.as_deref().unwrap_or("(none)");
                ("lock", format!("{file}: {primitive} {b} → {a}"))
            }
            HunkKind::Data { file, type_name, added_fields, removed_fields, renamed_fields } => (
                "data",
                format!(
                    "{file}: {type_name} +{}/-{} fields, {} renamed",
                    added_fields.len(),
                    removed_fields.len(),
                    renamed_fields.len()
                ),
            ),
            HunkKind::Docs { file, target, drift_kind } => (
                "docs",
                format!("{file}: {target} drift ({drift_kind})"),
            ),
            HunkKind::Deletion { file, entity_name, was_exported } => (
                "deletion",
                format!(
                    "{file}: {}{entity_name} removed",
                    if *was_exported { "exported " } else { "" }
                ),
            ),
        };
        out.push_str(&format!("    - {hid} ({kind}) {extras}\n"));
    }
    out
}

fn hunk_touches(_h: &floe_core::Hunk, _entity: &str) -> bool {
    // Heuristic placeholder: the Hunk struct references entities by
    // node id, not qualified name — resolving that cross-reference
    // needs the full artifact graph. For now split-subflows get the
    // cluster's entire entity set (the host re-trims downstream).
    // If the model emits `split`, which is rare, the subflow cards
    // may over-report entities until we wire in proper resolution.
    true
}

fn proposal_is_valid(p: &ClusterProposal, cluster: &Flow) -> bool {
    if p.name.trim().is_empty() || p.rationale.trim().is_empty() {
        return false;
    }
    let lower = p.name.to_lowercase();
    if RESERVED_NAMES.iter().any(|r| lower == *r) {
        return false;
    }
    if let Some(subs) = &p.split {
        if subs.is_empty() {
            return false;
        }
        let mut seen: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for s in subs {
            if s.name.trim().is_empty() || s.rationale.trim().is_empty() {
                return false;
            }
            if RESERVED_NAMES.iter().any(|r| s.name.to_lowercase() == *r) {
                return false;
            }
            for hid in &s.hunk_ids {
                if !cluster.hunk_ids.contains(hid) {
                    return false; // subflow referenced a hunk not in the cluster
                }
                if !seen.insert(hid.as_str()) {
                    return false; // duplicate hunk across subflows
                }
            }
        }
        // Every input hunk must end up somewhere.
        for hid in &cluster.hunk_ids {
            if !seen.contains(hid.as_str()) {
                return false;
            }
        }
    }
    true
}

#[derive(Debug, Clone, Deserialize)]
struct ClusterProposal {
    name: String,
    rationale: String,
    #[serde(default, deserialize_with = "deserialize_split")]
    split: Option<Vec<SubFlow>>,
}

#[derive(Debug, Clone, Deserialize)]
struct SubFlow {
    name: String,
    rationale: String,
    hunk_ids: Vec<String>,
}

/// Accept `split: false` (no split) or `split: [<sub>, …]` (split).
/// Anything else is a parse error.
fn deserialize_split<'de, D>(d: D) -> Result<Option<Vec<SubFlow>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let v = Value::deserialize(d)?;
    match v {
        Value::Bool(false) => Ok(None),
        Value::Null => Ok(None),
        Value::Array(_) => serde_json::from_value(v)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom(
            "split must be `false` or an array of sub-flows",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_plain_proposal() {
        let content = r#"{"name":"Streaming chunk API","rationale":"new streamChunk","split":false}"#;
        let p = extract_proposal(content).unwrap();
        assert_eq!(p.name, "Streaming chunk API");
        assert!(p.split.is_none());
    }

    #[test]
    fn parses_code_fenced_proposal() {
        let content = r#"```json
{"name":"Queue budget","rationale":"per-category caps","split":false}
```"#;
        let p = extract_proposal(content).unwrap();
        assert_eq!(p.name, "Queue budget");
    }

    #[test]
    fn parses_proposal_with_split_array() {
        let content = r#"{
          "name":"Mixed",
          "rationale":"contains two flows",
          "split":[
            {"name":"A","rationale":"aa","hunk_ids":["h1"]},
            {"name":"B","rationale":"bb","hunk_ids":["h2"]}
          ]
        }"#;
        let p = extract_proposal(content).unwrap();
        assert_eq!(p.split.as_ref().unwrap().len(), 2);
    }

    #[test]
    fn rejects_reserved_name() {
        let p = ClusterProposal {
            name: "misc".into(),
            rationale: "x".into(),
            split: None,
        };
        let cluster = cluster_with_ids(&["h1"]);
        assert!(!proposal_is_valid(&p, &cluster));
    }

    #[test]
    fn rejects_split_missing_hunk() {
        let p = ClusterProposal {
            name: "A".into(),
            rationale: "x".into(),
            split: Some(vec![SubFlow {
                name: "A1".into(),
                rationale: "x".into(),
                hunk_ids: vec!["h1".into()], // cluster has h1 + h2
            }]),
        };
        let cluster = cluster_with_ids(&["h1", "h2"]);
        assert!(!proposal_is_valid(&p, &cluster));
    }

    fn cluster_with_ids(ids: &[&str]) -> Flow {
        Flow {
            id: "flow-x".into(),
            name: "<structural: x>".into(),
            rationale: "r".into(),
            source: FlowSource::Structural,
            hunk_ids: ids.iter().map(|s| s.to_string()).collect(),
            entities: Vec::new(),
            extra_entities: Vec::new(),
            propagation_edges: Vec::new(),
            order: 0,
            evidence: Vec::new(),
            cost: None,
            intent_fit: None,
                            membership: None,
            proof: None,
        }
    }
}
