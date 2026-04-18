use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::cfg::CfgMap;
use crate::flow::Flow;
use crate::graph::Graph;
use crate::hunks::Hunk;

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
    /// always produces this list; the `adr` PI extension may replace it
    /// with LLM-validated flows.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub flows: Vec<Flow>,
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
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PrRef {
    pub repo: String,
    pub base_sha: String,
    pub head_sha: String,
}
