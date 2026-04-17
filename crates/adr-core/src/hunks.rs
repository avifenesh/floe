use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::graph::{EdgeId, NodeId};
use crate::provenance::Provenance;

/// The three semantic hunk types scope 1 delivers. The RFC lists more (lock, data,
/// docs, deletion); they land in later scopes without a schema bump — new variants
/// are additive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HunkKind {
    /// A call-graph edge appeared, disappeared, or moved.
    Call {
        added_edges: Vec<EdgeId>,
        removed_edges: Vec<EdgeId>,
    },
    /// A string-union state gained or lost variants, or transitions changed.
    State {
        node: NodeId,
        added_variants: Vec<String>,
        removed_variants: Vec<String>,
    },
    /// An exported API surface changed shape or a route handler moved.
    Api {
        node: NodeId,
        before_signature: Option<String>,
        after_signature: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Hunk {
    pub id: String,
    pub kind: HunkKind,
    pub provenance: Provenance,
}
