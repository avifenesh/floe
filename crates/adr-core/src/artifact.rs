use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

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
    pub hunks: Vec<Hunk>,
}

impl Artifact {
    pub fn new(pr: PrRef) -> Self {
        Self {
            schema_version: SCHEMA_VERSION.to_string(),
            pr,
            base: Graph::default(),
            head: Graph::default(),
            hunks: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct PrRef {
    pub repo: String,
    pub base_sha: String,
    pub head_sha: String,
}
