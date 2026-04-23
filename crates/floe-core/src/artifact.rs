use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cfg::CfgMap;
use crate::evidence::Axes;
use crate::flow::Flow;
use crate::graph::Graph;
use crate::hunks::Hunk;
use crate::inline_notes::InlineNote;
use crate::intent::IntentInput;

/// Where the cost-attribution pass stands for this artifact.
///
/// Cost comes from an LLM probe pass that runs asynchronously after the
/// synchronous pipeline finishes — we don't want the reviewer waiting on
/// 2 × probe latency just to see flows and evidence. `cost_status` lets
/// the frontend differentiate "no cost yet, try again shortly" from
/// "cost attribution is genuinely missing".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum CostStatus {
    /// Cost pass never ran. Either probe is disabled or the pipeline
    /// completed without reaching the cost stage.
    #[default]
    NotRun,
    /// Probe pass is mid-flight. UI should show "analysing…".
    Analyzing,
    /// Probe pass completed; every flow's `cost` field is populated.
    Ready,
    /// Probe pass errored. Partial cost may still be present on some
    /// flows; banner shown to the reviewer.
    Errored,
}


/// Where the parallel flow-naming (synth) pass stands for this
/// artifact. Mirrors [`CostStatus`] — synth runs as a background
/// task after READY so the workspace can open with structural flow
/// names immediately, refreshing to LLM names when the pass
/// completes (~30–90s on GLM-4.7).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum SynthStatus {
    /// No LLM configured; structural names are final.
    #[default]
    NotRun,
    /// Background synth in flight — flow names may update shortly.
    Analyzing,
    /// Background synth completed; LLM-named flows in place.
    Ready,
    /// Background synth errored; structural names stand, banner shown.
    Errored,
}


/// Where the intent-fit + proof-verification LLM passes stand for this
/// artifact. Mirrors [`CostStatus`] — proof lands asynchronously because
/// the passes run against GLM and each flow burns a cloud session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ProofStatus {
    /// No intent supplied, or the passes are disabled. Proof axis stays 0.
    #[default]
    NotRun,
    /// Intent + proof passes in flight. UI should show "analysing…".
    Analyzing,
    /// Passes completed; per-flow IntentFit + Proof claims populated.
    Ready,
    /// One or more sessions errored. Partial claims may be present;
    /// banner shown to the reviewer.
    Errored,
}


/// Schema version in the artifact frontmatter. Bump the minor on breaking changes
/// until v1, then follow semver.
pub const SCHEMA_VERSION: &str = "0.1.0";

/// The top-level JSON artifact emitted by `adr diff`. Every downstream consumer
/// (server, frontend, cost model, evidence collectors) reads this and only this.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Artifact {
    pub schema_version: String,
    pub pr: PrRef,
    pub base: Graph,
    pub head: Graph,
    /// Per-function CFGs for the base snapshot, keyed by Function `NodeId` in `base`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub base_cfg: CfgMap,
    /// Per-function CFGs for the head snapshot, keyed by Function `NodeId` in `head`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub head_cfg: CfgMap,
    pub hunks: Vec<Hunk>,
    /// Flows — groups of hunks that belong to one architectural story. The
    /// primary unit of review in v0.2. Deterministic structural clustering
    /// always produces this list; the LLM-synthesis pass (via `floe-mcp`) may replace it
    /// with LLM-validated flows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flows: Vec<Flow>,
    /// Status of the cost-attribution (probe) pass. See [`CostStatus`].
    #[serde(default)]
    pub cost_status: CostStatus,
    /// Status of the parallel flow-naming pass. See [`SynthStatus`].
    /// Initially `Analyzing` when an LLM is configured; flips to
    /// `Ready` once every cluster's proposal is merged. The FE uses
    /// this to decide whether to keep polling for flow-name updates
    /// after the workspace opens.
    #[serde(default)]
    pub synth_status: SynthStatus,
    /// Status of the intent-fit + proof-verification LLM passes. See
    /// [`ProofStatus`]. Independent of `cost_status` — probe runs local,
    /// proof runs cloud (GLM), they spawn in parallel.
    #[serde(default)]
    pub proof_status: ProofStatus,
    /// Repo-wide baseline summary from the probe pass. Populated by
    /// `floe-cost` alongside per-flow `Cost`; carries the denominators the
    /// frontend needs to render bars as *percent-of-baseline* rather than
    /// *relative rank*. `None` until the probe pass lands.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline: Option<ArtifactBaseline>,
    /// What the PR claims to accomplish — consumed by the intent-fit and
    /// proof-verification LLM passes. Either pre-structured by the
    /// caller or raw PR text the passes structure on the fly. `None`
    /// when the caller didn't supply one (passes then emit a
    /// "no-intent" claim and proof stays at 0).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<IntentInput>,
    /// Side-channel notes from the reviewer — pasted benchmark output,
    /// staging logs, corroborating screenshots turned into text. The
    /// proof-verification pass reads this alongside the code when
    /// deciding whether a claim has evidence.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub notes: String,
    /// LLM-generated 1–2 sentence summary of the stated intent. Populated
    /// once at the start of the proof pipeline so the reviewer doesn't
    /// have to read the raw PR description; only present when intent
    /// arrived as `RawText` and the GLM call succeeded.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent_summary: Option<String>,
    /// LLM-derived reviewer-facing title + optional short description
    /// for the PR itself — replaces `repo#N` as the scannable heading
    /// in the UI. Populated by the summary pass (GLM-4.6 by default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pr_summary: Option<PrSummary>,
    /// Reviewer-facing notices — short, one-line status hints surfaced
    /// as toasts in the UI. Emitted by passes that degrade gracefully
    /// (e.g. intent extraction when ollama is unreachable) so the
    /// reviewer understands *why* a downstream axis is empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<String>,
    /// Compile-unit delta from the Phase B `tsc --noEmit` pass — a
    /// type-checker-authoritative read on "did this PR break or fix
    /// anything?". `None` when the pass is disabled or couldn't run
    /// (no tsc on PATH, no tsconfig at root).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compile_diagnostics: Option<crate::evidence::CompileDelta>,
    /// Test-run delta — captured outcome of `FLOE_TEST_CMD` on base
    /// and head. `None` when unconfigured or skipped. See RFC
    /// Appendix F upgrade #4.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub test_run: Option<crate::evidence::ExternalRunDelta>,
    /// Bench-run delta — `FLOE_BENCH_CMD`. Structure mirrors test_run.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bench_run: Option<crate::evidence::ExternalRunDelta>,
    /// Per-file line-coverage delta derived from an lcov/json-summary
    /// file scanned at base and head around the `FLOE_TEST_CMD` run.
    /// `None` when no coverage file was found.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub coverage_delta: Option<crate::evidence::CoverageDelta>,
    /// Reviewer's explicit verdict on this PR — "approve", "request
    /// changes", or "comment" (neutral leave-a-note stance). Distinct
    /// from inline notes (annotations) and from ship-readiness (an
    /// algorithmic rollup). Persisted with the artifact so a reviewer
    /// sees their own stance on repeat visits and a second reviewer
    /// sees the first's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_verdict: Option<ReviewVerdictRecord>,
    /// Reviewer notes anchored to specific objects (hunks, flows,
    /// entities, intent claims, or file lines). Written via
    /// `/analyze/:id/notes` and bundled with their object's context
    /// by `/analyze/:id/notes/export` for downstream coding-agent use.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub inline_notes: Vec<InlineNote>,
}

/// Base-side totals used as denominators for the UI's %-of-baseline bars,
/// plus the model pin RFC v0.3 §9 requires on every comparison.
///
/// Per-axis cost is summed across every entity in the base probe run on
/// the axis's probe (e.g. `continuation = Σ base.per_probe_entity_cost["api-surface"]`).
/// `tokens_base` is the base probe run's total token usage; `tokens_head`
/// is the head run's, used for the PR-level "token cost moved by X%"
/// headline.
///
/// The pin fields (`probe_model`, `synthesis_model`, `proof_model`) let
/// [`ArtifactBaseline::pin_matches`] refuse an apples-to-oranges compare
/// when any of the three LLM passes shifted between two artifacts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ArtifactBaseline {
    /// Per-axis base-run cost. All three fields non-negative.
    pub axes_base: Axes,
    pub tokens_base: u32,
    pub tokens_head: u32,
    pub probe_model: String,
    pub probe_set_version: String,
    /// Flow-synthesis model, e.g. `"glm-4.7"` or `"qwen3.5:27b-q4_K_M"`.
    /// `None` when the artifact shipped with structural clustering only
    /// (no LLM synthesis ran).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub synthesis_model: Option<String>,
    /// Proof-verification model — always GLM when set (proof pass refuses
    /// non-GLM backends without loud warning, see
    /// `llm::config::from_env_proof`). `None` when proof pass was skipped.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proof_model: Option<String>,
}

impl ArtifactBaseline {
    /// Does `self` have the same pin as `other`? Checks all three model
    /// fields + the probe-set version. Returns `false` if ANY differ —
    /// callers treat that as "re-baseline required" per RFC v0.3 §9.
    ///
    /// `None == None` counts as matching (neither side ran that pass).
    /// `Some(a) != Some(b)` obviously differs. `None != Some(_)` also
    /// differs — an artifact with a proof pass and one without are not
    /// comparable even if they happen to agree on probe output.
    pub fn pin_matches(&self, other: &Self) -> bool {
        self.probe_model == other.probe_model
            && self.probe_set_version == other.probe_set_version
            && self.synthesis_model == other.synthesis_model
            && self.proof_model == other.proof_model
    }
}

impl Artifact {
    pub fn new(pr: PrRef) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            pr,
            base: Graph::default(),
            head: Graph::default(),
            base_cfg: CfgMap::default(),
            head_cfg: CfgMap::default(),
            hunks: Vec::new(),
            flows: Vec::new(),
            cost_status: CostStatus::default(),
            synth_status: SynthStatus::default(),
            proof_status: ProofStatus::default(),
            baseline: None,
            intent: None,
            notes: String::new(),
            intent_summary: None,
            pr_summary: None,
            notices: Vec::new(),
            compile_diagnostics: None,
            test_run: None,
            bench_run: None,
            coverage_delta: None,
            review_verdict: None,
            inline_notes: Vec::new(),
        }
    }

    /// Clone the artifact with one graph blanked out — used by the
    /// probe pass, which needs a "base-only" or "head-only" view of the
    /// repo to answer navigation questions. Hunks and flows are cleared
    /// since they're diff-shaped and meaningless against one snapshot.
    pub fn side_only(&self, side: Side) -> Self {
        let mut out = self.clone();
        match side {
            Side::Base => {
                out.head = Graph::default();
                out.head_cfg = CfgMap::default();
            }
            Side::Head => {
                out.base = Graph::default();
                out.base_cfg = CfgMap::default();
            }
        }
        out.hunks.clear();
        out.flows.clear();
        out.cost_status = CostStatus::NotRun;
        out.synth_status = SynthStatus::NotRun;
        out.proof_status = ProofStatus::NotRun;
        out.baseline = None;
        out.intent = None;
        out.notes.clear();
        out.intent_summary = None;
        out.pr_summary = None;
        out.compile_diagnostics = None;
        out.test_run = None;
        out.bench_run = None;
        out.review_verdict = None;
        out.inline_notes.clear();
        out
    }

    /// Derive a stable SHA for one side of the repo — used as the
    /// baseline key. Walks every node on the given side, sorts by
    /// `(file, qualified name, provenance hash)`, and blake3s the
    /// concatenation. Deterministic, git-free, and re-computes cheaply.
    pub fn snapshot_sha(&self, side: Side) -> String {
        use crate::graph::NodeKind;
        let graph = match side {
            Side::Base => &self.base,
            Side::Head => &self.head,
        };
        let mut rows: Vec<String> = graph
            .nodes
            .iter()
            .map(|n| {
                let qname = match &n.kind {
                    NodeKind::Function { name, .. }
                    | NodeKind::Type { name }
                    | NodeKind::State { name, .. } => name.clone(),
                    NodeKind::ApiEndpoint { method, path } => format!("{method} {path}"),
                    NodeKind::File { path } => path.clone(),
                };
                format!("{}|{qname}|{}", n.file, n.provenance.hash)
            })
            .collect();
        rows.sort();
        let blob = rows.join("\n");
        blake3::hash(blob.as_bytes()).to_hex().to_string()
    }
}

/// Which snapshot side a helper operates on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Base,
    Head,
}

/// Reviewer verdict — one of three stances. `Comment` means "I looked
/// and I want to leave a note but not gate the PR"; distinct from no
/// record at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ReviewVerdict {
    Approve,
    RequestChanges,
    Comment,
}

/// Persisted verdict — who set it and when. Opaque `author` (GitHub
/// login when OAuth is on, `"local"` otherwise), RFC3339 timestamp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ReviewVerdictRecord {
    pub verdict: ReviewVerdict,
    pub author: String,
    pub set_at: String,
}

/// LLM-derived reviewer-facing title (+ optional short description)
/// for the PR. Shown in the top spine and sidebar in place of the raw
/// `repo#N` identifier. `description` is only set when the LLM judges
/// it adds signal beyond the headline — clean surface wins.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct PrSummary {
    /// One-line reviewer-facing title (≤60 chars).
    pub headline: String,
    /// Optional 1–2 sentence description. Omitted when the headline
    /// already tells the whole story.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Model tag that produced this summary (e.g. `"glm-4.6"`).
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PrRef {
    pub repo: String,
    pub base_sha: String,
    pub head_sha: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::{Node, NodeId, NodeKind, Span};
    use crate::provenance::Provenance;

    fn node(id: u32, name: &str, file: &str, hash: &str) -> Node {
        Node {
            id: NodeId(id),
            kind: NodeKind::Function {
                name: name.into(),
                signature: String::new(),
            },
            file: file.into(),
            span: Span { start: 0, end: 1 },
            provenance: Provenance {
                source: "t".into(),
                version: "0".into(),
                pass_id: "p".into(),
                hash: hash.into(),
            },
            package: None,
        }
    }

    fn seed() -> Artifact {
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.base.nodes.push(node(1, "A.x", "src/a.ts", "aaa"));
        a.base.nodes.push(node(2, "A.y", "src/a.ts", "bbb"));
        a.head.nodes.push(node(3, "A.x", "src/a.ts", "ccc")); // same qname, different hash
        a.head.nodes.push(node(4, "B.z", "src/b.ts", "ddd"));
        a
    }

    #[test]
    fn side_only_blanks_the_other_graph() {
        let a = seed();
        let base = a.side_only(Side::Base);
        assert_eq!(base.base.nodes.len(), 2);
        assert_eq!(base.head.nodes.len(), 0);
        assert!(base.hunks.is_empty());
        assert!(base.flows.is_empty());
        assert_eq!(base.cost_status, CostStatus::NotRun);

        let head = a.side_only(Side::Head);
        assert_eq!(head.head.nodes.len(), 2);
        assert_eq!(head.base.nodes.len(), 0);
    }

    #[test]
    fn snapshot_sha_is_deterministic_and_side_sensitive() {
        let a = seed();
        let base1 = a.snapshot_sha(Side::Base);
        let base2 = a.snapshot_sha(Side::Base);
        let head1 = a.snapshot_sha(Side::Head);
        assert_eq!(base1, base2, "same inputs → same sha");
        assert_ne!(base1, head1, "base and head differ (different hashes)");
        // Content change → sha change.
        let mut b = a.clone();
        b.base.nodes[0].provenance.hash = "zzz".into();
        assert_ne!(base1, b.snapshot_sha(Side::Base));
    }

    #[test]
    fn cost_status_default_is_not_run() {
        let a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        assert_eq!(a.cost_status, CostStatus::NotRun);
    }

    fn baseline_with(
        synthesis: Option<&str>,
        proof: Option<&str>,
        probe: &str,
    ) -> ArtifactBaseline {
        ArtifactBaseline {
            axes_base: Axes::default(),
            tokens_base: 0,
            tokens_head: 0,
            probe_model: probe.into(),
            probe_set_version: "0.1".into(),
            synthesis_model: synthesis.map(|s| s.into()),
            proof_model: proof.map(|s| s.into()),
        }
    }

    #[test]
    fn pin_matches_on_same_three_models() {
        let a = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "qwen3.5:27b-q4_K_M");
        let b = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "qwen3.5:27b-q4_K_M");
        assert!(a.pin_matches(&b));
    }

    #[test]
    fn pin_mismatch_on_synthesis_drift() {
        // User switched from the cloud default to the local fallback
        // between runs — cost numbers aren't comparable even if the
        // probe model was identical.
        let a = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "qwen3.5:27b-q4_K_M");
        let b = baseline_with(
            Some("qwen3.5:27b-q4_K_M"),
            Some("glm-4.7"),
            "qwen3.5:27b-q4_K_M",
        );
        assert!(!a.pin_matches(&b));
    }

    #[test]
    fn pin_mismatch_when_proof_ran_on_one_side_only() {
        // Intent supplied on run A but not run B — proof ran once,
        // skipped once. Not apples-to-apples.
        let a = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "qwen3.5:27b-q4_K_M");
        let b = baseline_with(Some("glm-4.7"), None, "qwen3.5:27b-q4_K_M");
        assert!(!a.pin_matches(&b));
    }

    #[test]
    fn pin_matches_when_both_skipped_proof() {
        let a = baseline_with(Some("glm-4.7"), None, "qwen3.5:27b-q4_K_M");
        let b = baseline_with(Some("glm-4.7"), None, "qwen3.5:27b-q4_K_M");
        assert!(a.pin_matches(&b));
    }

    #[test]
    fn pin_mismatch_on_probe_model_drift() {
        let a = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "qwen3.5:27b-q4_K_M");
        let b = baseline_with(Some("glm-4.7"), Some("glm-4.7"), "glm-4.5-air");
        assert!(!a.pin_matches(&b));
    }
}
