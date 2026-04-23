//! Evidence schema. A claim is a one-line assertion about a flow with a
//! strength tier and provenance. Collectors live in the `floe-evidence`
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
    /// A file this flow touches lost line-coverage between base and head
    /// (derived from lcov / vitest json-summary). Strength reflects the
    /// drop size. Produced by `floe-server` coverage pass.
    CoverageDrop,
}

/// Per-flow review-effort estimate, emitted by the `floe-cost` pass from
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

/// Compile-unit delta — what the type checker says about base vs head.
///
/// Produced by the Phase B compile pass (`tsc --noEmit` run at each
/// side). The diff classifies every diagnostic:
///
/// - **new_on_head** — present on head, absent on base. High-strength
///   architectural claim; reviewer sees "this PR introduced a type
///   error".
/// - **resolved_on_head** — present on base, absent on head. Positive
///   signal; the PR fixed a pre-existing error.
/// - **persistent** — on both sides. Observation, not a claim against
///   this PR; useful for reviewers who want to know the repo was
///   already in a broken state.
///
/// Emitted as `Some(...)` when the pass ran, `None` when
/// `FLOE_COMPILE_PASS=0` disables it or the repo has no runnable
/// TypeScript compiler.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CompileDelta {
    /// TypeScript compiler version reported on the head side.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub compiler_version: String,
    pub new_on_head: Vec<CompileDiagnostic>,
    pub resolved_on_head: Vec<CompileDiagnostic>,
    pub persistent: Vec<CompileDiagnostic>,
    /// True when `tsc` finished cleanly on both sides; false when the
    /// pass hit an infrastructural failure (missing tsconfig, timeout)
    /// and the diff is unreliable. UI surfaces a caveat when false.
    pub both_ran: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct CompileDiagnostic {
    pub file: String,
    pub line: u32,
    pub column: u32,
    pub code: String,
    pub severity: DiagnosticSeverity,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

/// Captured outcome of an external runner (`FLOE_TEST_CMD`,
/// `FLOE_BENCH_CMD`) invoked at one side of the PR.
///
/// The command is whatever the user configures — `vitest run`, `cargo
/// test`, `bun test`, `tinybench`, etc. We keep the raw output around
/// (truncated to a bounded size) so the proof pass can mine it and
/// the reviewer can click to inspect it. Exit status + timing are the
/// key deterministic signals the evidence layer reasons on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalRunOutcome {
    /// 0 = success. Non-zero surfaces as a failing build/run.
    pub exit_code: i32,
    pub duration_ms: u64,
    /// Truncated stdout (bounded at ~64 KB so large bench runs don't
    /// bloat the artifact).
    pub stdout: String,
    pub stderr: String,
    /// True when the captured bytes were truncated — UI shows a caveat.
    pub truncated: bool,
}

/// Result of running an external tests/bench command on both sides.
///
/// `both_ran` is true when each side returned cleanly (even with a
/// non-zero exit — the command ran, it just said "tests failed"). It
/// becomes false only when we couldn't execute the command at all
/// (binary missing, timeout).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ExternalRunDelta {
    /// Human-readable command that was run; used in the UI caveat.
    pub command: String,
    pub base: Option<ExternalRunOutcome>,
    pub head: Option<ExternalRunOutcome>,
    pub both_ran: bool,
}

/// Coverage delta derived from an `lcov.info` (or vitest
/// `json-summary.json`) file scanned at base and head after the tests
/// run. Per-file line-coverage percentages — the reviewer sees which
/// files *lost* coverage in this PR.
///
/// `files` contains only files with a non-zero delta OR a head entry;
/// purely-unchanged files are elided to keep the artifact compact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageDelta {
    /// Absolute path (or artifact-relative path) to the lcov / summary
    /// file that produced this delta. Surfaced in the UI caveat.
    pub source: String,
    pub files: Vec<CoverageFile>,
}

/// Per-file line-coverage values encoded as **permille** (0–1000) so the
/// struct can keep `Eq` (needed to derive Eq on Artifact). Convert to
/// percent by dividing by 10 in UI code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CoverageFile {
    pub file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_permille: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub head_permille: Option<i32>,
    /// `head - base` in permille. `None` when either side absent.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_permille: Option<i32>,
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

/// Source range a claim points at — lifted from LSP responses (Phase
/// D) or from diagnostic coordinates (compile pass). Gives the UI
/// enough to jump straight to the exact token the claim cites.
///
/// `side` is `base` or `head`; `line`/`column` are 1-indexed to match
/// TypeScript compiler output and the conventional editor UX (LSP is
/// 0-indexed but we normalise here). `length` is optional — when
/// absent, UI highlights the whole line.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct SourceRef {
    pub file: String,
    #[serde(rename = "side")]
    pub side: SourceSide,
    pub line: u32,
    pub column: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum SourceSide {
    Base,
    Head,
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
    /// Source ranges this claim cites. Compile diagnostics carry the
    /// exact `(file, line, col)` from `tsc`; LSP-backed claims resolve
    /// reference positions via `textDocument/references`. The UI uses
    /// these to render "jump to source" affordances inline. Empty is
    /// valid — older claims and flow-global observations won't have
    /// anchors. See RFC Appendix F upgrade #6.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub source_refs: Vec<SourceRef>,
}
