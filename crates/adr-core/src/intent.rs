//! Intent schema — what the PR claims to accomplish and the evidence
//! that backs those claims. Fed into two LLM passes downstream:
//!
//! - **intent-fit** (per flow) — does this flow deliver something the
//!   intent mentions? Emits [`ClaimKind::IntentFit`] claims.
//! - **proof-verification** (per flow) — is there evidence for the
//!   intent's claims? Emits [`ClaimKind::Proof`] claims and fills
//!   [`crate::evidence::Axes::proof`].
//!
//! The reviewer's free-text PR description can be used directly via
//! [`IntentInput::RawText`]; an early LLM pass can structure it into an
//! [`Intent`] for the downstream passes to consume.
//!
//! See `feedback_proof_not_tests.md` and `project_cost_model.md` for the
//! product shape — proof is evidence of *stated intent*, not unit-test
//! presence; unit tests are at best a weak context signal.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::evidence::Strength;

/// The kind of evidence a claim expects. Drives how the
/// proof-verification pass looks for corroborating artefacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EvidenceType {
    /// A benchmark run — numbers in the PR body, perf log, etc.
    Bench,
    /// An `examples/` file (or similar) exercising the claim end-to-end.
    Example,
    /// A test that **asserts the specific claim** (not just touches the
    /// function — see the RFC).
    Test,
    /// A stated observation — "p99 dropped 40% in staging". Weakest form
    /// of proof; still counts if the reviewer's notes corroborate.
    Observation,
}

/// One claim a PR makes about itself.
///
/// Example: `"streams now back-pressure at 64 KB" / EvidenceType::Bench`
/// with `detail = "ran bin/smoke-stream.sh — head p99 dropped from
/// 180ms to 72ms"`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IntentClaim {
    /// The claim itself in one sentence. Reviewer-facing copy.
    pub statement: String,
    pub evidence_type: EvidenceType,
    /// Optional human-readable detail — benchmark output, example path,
    /// test file reference, observation context. Reviewer pastes this
    /// into intent.json or the PR body.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub detail: String,
}

/// Structured PR intent. This is the shape the intent-fit and
/// proof-verification passes consume. Raw text intents are fed through
/// a pre-pass that reshapes them into this form.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Intent {
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<IntentClaim>,
}

/// How the caller supplied intent. `Structured` is the canonical shape
/// the downstream passes want; `RawText` is the author's PR description
/// verbatim — structured on the fly before the passes run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum IntentInput {
    Structured(Intent),
    RawText(String),
}

impl IntentInput {
    /// Short preview for logs and UI when the caller doesn't care which
    /// shape was supplied.
    pub fn preview(&self) -> String {
        match self {
            IntentInput::Structured(i) => i.title.clone(),
            IntentInput::RawText(s) => {
                let trimmed = s.trim();
                if trimmed.len() > 80 {
                    format!("{}…", &trimmed[..77])
                } else {
                    trimmed.to_string()
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// Intent-fit & proof results — produced by the `adr-server` LLM passes
// and attached to each `Flow`. Deliberately not part of `Cost.axes` —
// proof is its own product section, not a cost dimension.
// ─────────────────────────────────────────────────────────────────────

/// Intent-fit verdict for one flow — does this flow deliver something
/// the PR's stated intent mentions?
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum IntentFitVerdict {
    /// The flow clearly delivers one or more intent claims.
    Delivers,
    /// The flow touches the intent's area but doesn't close the loop —
    /// partial progress, further work needed.
    Partial,
    /// The flow is off-topic relative to the stated intent (potential
    /// scope-creep or unrelated side-change).
    Unrelated,
    /// No intent was supplied; the pass short-circuits.
    NoIntent,
}

/// Per-flow intent-fit result from the intent-fit LLM pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct IntentFit {
    pub verdict: IntentFitVerdict,
    pub strength: Strength,
    /// One-paragraph justification the reviewer can scan.
    pub reasoning: String,
    /// Indices into the intent's `claims[]` that this flow addresses.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub matched_claims: Vec<usize>,
    pub model: String,
    pub prompt_version: String,
}

/// Proof verdict for one flow — aggregate over all intent-claims the
/// proof-verification pass checked against this flow's code + reviewer notes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProofVerdict {
    /// Proof found for the claim(s) — benchmarks quoted, examples
    /// present, claim-asserting tests exist.
    Strong,
    /// Proof exists for some claim(s) but not others, or evidence is
    /// indirect (test exercises the function but doesn't assert the
    /// specific claim).
    Partial,
    /// Intent claims exist but no evidence backs them.
    Missing,
    /// No intent was supplied; the pass short-circuits.
    NoIntent,
}

/// Status of a single claim's proof search, emitted by the LLM and
/// surfaced per-claim in the UI so the reviewer can see exactly which
/// claim is unverified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClaimProofStatus {
    /// Index into `Intent.claims[]` — -1 when the LLM structured a
    /// claim out of raw text and doesn't map to a pre-structured index.
    pub claim_index: i32,
    /// Claim statement echo, so the UI can render without re-resolving.
    pub statement: String,
    /// Whether the LLM found proof for *this specific claim*.
    pub status: ClaimProofKind,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub evidence: Vec<ProofEvidence>,
    pub strength: Strength,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ClaimProofKind {
    Found,
    Partial,
    Missing,
}

/// One piece of evidence the LLM cited for a claim.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ProofEvidence {
    pub evidence_type: EvidenceType,
    /// What the LLM actually found — benchmark line, example path, test
    /// name, reviewer-note excerpt. Human-readable, reviewer-facing.
    pub detail: String,
    /// Optional path into the repo when the evidence is a file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// Per-flow proof-verification result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Proof {
    pub verdict: ProofVerdict,
    pub strength: Strength,
    /// One-paragraph reviewer-facing summary — "2 of 3 claims verified
    /// via examples/stream-backpressure.ts and the pasted benchmark; the
    /// retry-limit claim has no evidence."
    pub reasoning: String,
    /// Per-claim breakdown. Empty when `verdict = NoIntent`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub claims: Vec<ClaimProofStatus>,
    pub model: String,
    pub prompt_version: String,
}

