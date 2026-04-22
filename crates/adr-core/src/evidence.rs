//! Evidence schema. A claim is a one-line assertion about a flow with a
//! strength tier and provenance. Collectors live in the `adr-evidence`
//! crate; the schema lives here because [`crate::Flow`] carries a
//! `Vec<Claim>` directly so every downstream consumer (server, frontend,
//! cost model) reads claims from the same place.
//!
//! Claim text is reviewer-facing copy. Strength is coarse (three tiers)
//! on purpose — the reviewer wants a traffic-light, not a percentile.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::Provenance;

/// The reviewer-visible strength of a claim. Three tiers, tuned to the
/// scan rate of a human reading a PR: "I trust this", "worth a look",
/// "gesture only".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum Strength {
    High,
    Medium,
    Low,
}

/// Kind of claim. Drives the icon / color the frontend picks. Kept
/// coarse so the enum stays stable as collectors are added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimKind {
    /// All API hunks in the flow share a signature shape (parameter
    /// added, return type widened, etc.).
    SignatureConsistency,
    /// The Call hunks in the flow form a connected call chain rather
    /// than independent edges.
    CallChain,
    /// Hunks touch N files (often a "yes, but watch the fanout" signal).
    CrossFile,
    /// Test files touch entities in this flow (or don't).
    TestCoverage,
    /// The flow sits entirely within one file — review scope is tight.
    SingleFile,
    /// LLM intent-fit verdict: this flow delivers (or doesn't) something
    /// the PR's stated intent mentions. Produced by the intent-fit pass.
    IntentFit,
    /// LLM proof-verification verdict: an intent-claim has (or lacks)
    /// corresponding evidence — a benchmark result, an example file,
    /// a test that asserts the claim, or a corroborating observation
    /// from reviewer notes. Produced by the proof-verification pass.
    /// Drives [`Axes::proof`].
    Proof,
    /// Generic observation — used when no more specific kind applies.
    Observation,
}

/// Per-flow review-effort estimate, emitted by the `adr-cost` pass from
/// the probe baselines (see `docs/scope-5-cost-model.md`).
///
/// `net` is **signed**: negative means the PR made the affected flow
/// cheaper for the next LLM session to navigate (a refactor that dropped
/// complexity), positive means harder. Scale is "arbitrary cost units"
/// from the probe — comparable across flows of one PR and across PRs on
/// the same repo, not across repos.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cost {
    pub net: i32,
    pub axes: Axes,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub drivers: Vec<CostDriver>,
    /// Signed token-usage delta for this flow — `Σ (head.tokens - base.tokens)`
    /// across the flow's entities. Translates directly to API-billing impact
    /// for users running LLM sessions against the head repo.
    #[serde(default)]
    pub tokens_delta: i32,
    /// Which probe model produced the baselines these deltas came from.
    /// Stamped so a reviewer can ask "why did the number move?" when a
    /// probe-model change drifts the baseline.
    pub probe_model: String,
    pub probe_set_version: String,
}

/// Signed per-axis breakdown — each axis accumulates the contribution
/// from the probe assigned to it. v0 assignment:
///
/// | axis          | source probe              |
/// |---------------|---------------------------|
/// | continuation  | `probe-api-surface`       |
/// | operational   | `probe-external-boundaries` |
/// | runtime       | `probe-type-callsites`    |
///
/// Proof intentionally lives *outside* the cost axes — see
/// [`crate::intent::Proof`] on each [`crate::Flow`]. Cost is navigation
/// movement; proof is evidence of stated intent. Avi's call: mixing
/// them confuses the reviewer about what each bar is saying.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, JsonSchema)]
pub struct Axes {
    pub continuation: i32,
    pub runtime: i32,
    pub operational: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CostDriver {
    /// Short reviewer-facing label — "api-surface probe", "type-callsites probe".
    pub label: String,
    /// Signed contribution to `net`. Drivers sum to `net`.
    pub value: i32,
    /// Optional expanded detail — e.g. a sample entity or per-axis note.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// A single claim backing (or cautioning about) a flow's rationale.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Claim {
    /// Stable id — `claim-<blake3>` over `(flow_id, kind, text)`. Lets
    /// the frontend key rows without mutating positions across re-runs.
    pub id: String,
    /// Human-readable one-liner. The reviewer's eye goes here first.
    pub text: String,
    pub kind: ClaimKind,
    pub strength: Strength,
    /// Qualified entity names that participate in this claim. Empty is
    /// valid for flow-global observations.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entities: Vec<String>,
    /// Which pass produced this claim — lets the UI answer "where did
    /// this come from?" without guessing.
    pub provenance: Provenance,
}
