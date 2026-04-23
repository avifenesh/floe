use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::evidence::{Claim, Cost};
use crate::intent::{IntentFit, Proof};

/// A flow — the primary unit of review in v0.3.
///
/// The structural floor produces flows with `FlowSource::Structural`. The
/// LLM-synthesis pass (driven by `floe-server` over the `floe-mcp` stdio
/// tool contract, GLM-4.7 cloud by default or Qwen 3.5 27B local as the
/// offline fallback) replaces the list with LLM-validated flows tagged
/// `FlowSource::Llm`. If the LLM run is rejected, we ship the structural
/// list unchanged and the UI surfaces a "structural only" banner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Flow {
    /// Stable id — `flow-<blake3>` computed from the bucket name and
    /// hunk-id list. Two runs of the structural clustering on the same
    /// artifact produce identical ids.
    pub id: String,
    /// Reviewer-facing name. Structural flows use `<structural: <bucket>>`;
    /// LLM flows use an intent-shaped name.
    pub name: String,
    /// One-or-two-sentence explanation of why these hunks are together.
    pub rationale: String,
    /// Provenance of this flow — structural or LLM-synthesised.
    pub source: FlowSource,
    /// Every hunk that belongs to this flow. A hunk may appear in multiple
    /// flows (explicitly allowed). Every hunk in the artifact must appear
    /// in at least one flow (host-enforced when the LLM produces flows;
    /// guaranteed by construction when structural).
    pub hunk_ids: Vec<String>,
    /// Qualified entity names participating in this flow. Kept as strings
    /// so they match across base/head snapshots.
    pub entities: Vec<String>,
    /// Entities the LLM added beyond the hunk-derived set — unchanged
    /// callers/callees the reviewer should see in context. Empty for
    /// structural flows.
    pub extra_entities: Vec<String>,
    /// Unchanged call-sites / refs that reach this flow's entities
    /// (1-hop in v0). Each tuple is `(from_qualified_name,
    /// to_qualified_name)` where at least one endpoint is in
    /// `entities`. The frontend renders these as dashed context
    /// arrows so the reviewer sees "who else calls into this flow"
    /// without the noise of the full graph.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub propagation_edges: Vec<(String, String)>,
    /// Render order — stable in structural output; LLM runs preserve
    /// whatever order the host finalises.
    pub order: u32,
    /// Per-flow evidence — claims that back or caution about the flow's
    /// rationale. Populated by the `floe-evidence` pass after flows are
    /// finalised; may be empty for a flow with no extractable claims.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<Claim>,
    /// Per-flow review-effort estimate. Populated by the `floe-cost`
    /// pass; `None` when the pass hasn't run or errored.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cost: Option<Cost>,
    /// Intent-fit verdict for this flow — does it deliver something the
    /// PR's stated intent claims? Populated by the intent-fit LLM pass
    /// (see `docs/scope-5-cost-model.md` and `feedback_proof_uses_glm.md`).
    /// `None` when no intent was supplied or the pass hasn't run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_fit: Option<IntentFit>,
    /// Proof-verification result for this flow — is there evidence
    /// backing the intent's claims? Populated by the proof-verification
    /// LLM pass. Independent of `cost` — proof is its own section in
    /// the product, not a cost dimension.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof: Option<Proof>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum FlowSource {
    /// Structural clustering ran. No LLM involved or LLM output rejected.
    Structural,
    /// LLM synthesis ran, validated, and was accepted by the host.
    Llm {
        model: String,
        version: String,
    },
}
