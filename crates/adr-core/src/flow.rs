use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// A flow — the primary unit of review in v0.2. Lives at
/// `artifact.flows[]` (schema update lands with this crate).
///
/// The structural floor produces flows with `FlowSource::Structural`. The
/// `adr` PI extension replaces the list with LLM-validated flows tagged
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
    /// Render order — stable in structural output; LLM runs preserve
    /// whatever order the host finalises.
    pub order: u32,
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
