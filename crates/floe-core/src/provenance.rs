use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Every node, edge, and hunk carries provenance so the UI can answer
/// "where did this fact come from?" without guessing.
///
/// `source` names the pass that produced the fact (e.g. `tree-sitter-typescript`,
/// `scip-typescript`, `floe-hunks/call`). `version` is the tool version at the time
/// of the run. `pass_id` is stable across a single analysis; `hash` is a blake3 of
/// the raw input bytes the pass consumed — used to detect stale cached facts.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Provenance {
    pub source: String,
    pub version: String,
    pub pass_id: String,
    pub hash: String,
}

impl Provenance {
    pub fn new(
        source: impl Into<String>,
        version: impl Into<String>,
        pass_id: impl Into<String>,
        input: &[u8],
    ) -> Self {
        Self {
            source: source.into(),
            version: version.into(),
            pass_id: pass_id.into(),
            hash: blake3::hash(input).to_hex().to_string(),
        }
    }
}
