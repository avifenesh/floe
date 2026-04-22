//! Request and response shapes as the LLM sees them over the wire.
//!
//! One departure from `docs/adr-pi-extension.md`: the doc's example uses
//! opaque `"node-<hex>"` strings for entity ids. We use **qualified names**
//! (e.g. `"Queue.setBudget"`) directly, because:
//!
//! 1. `Flow.entities` in the artifact is already a qualified-name list.
//! 2. The LLM reads source code with qualified names; round-tripping them
//!    through an opaque id buys nothing.
//! 3. Qualified names are stable across `base` and `head` — opaque ids are
//!    per-snapshot.

use serde::{Deserialize, Serialize};

/// Which side(s) of the diff a hunk touches.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Side {
    Added,
    Removed,
    Both,
}

/// `adr:list_hunks()` result item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HunkSummary {
    pub id: String,
    pub kind: HunkKindTag,
    pub summary: String,
    /// Qualified names of entities directly touched by this hunk.
    pub entities: Vec<String>,
    pub side: Side,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HunkKindTag {
    Call,
    State,
    Api,
}

/// `adr:get_entity(id)` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityDescriptor {
    /// Qualified name.
    pub id: String,
    pub kind: EntityKindTag,
    pub name: String,
    pub file: String,
    /// Byte span in the file. The contract doc sketches line-based spans,
    /// but the artifact stores bytes; we pass bytes through unchanged so the
    /// renderer (or the LLM's `read` tool) can convert.
    pub span: SpanDto,
    pub side: SnapshotSide,
    /// Only present for function / method entities.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum EntityKindTag {
    Function,
    Type,
    State,
    ApiEndpoint,
    File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnapshotSide {
    Base,
    Head,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SpanDto {
    pub start: u32,
    pub end: u32,
}

/// `adr:neighbors(id, hops)` result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborsResponse {
    pub nodes: Vec<EntityDescriptor>,
    pub edges: Vec<NeighborEdge>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NeighborEdge {
    pub from: String,
    pub to: String,
    pub kind: NeighborEdgeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tag", rename_all = "kebab-case")]
pub enum NeighborEdgeKind {
    Calls,
    Defines,
    Exports,
    /// State transition between two variants of the same state-union node.
    Transitions { from: String, to: String },
}

/// `adr:list_flows_initial()` result item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FlowInitial {
    pub id: String,
    pub name: String,
    pub rationale: String,
    pub hunk_ids: Vec<String>,
    pub entities: Vec<String>,
    /// Always `"structural"` for v0 — kept as a string so future LLM
    /// confidence tiers (`"high"`, `"medium"`, `"low"`) slot in cleanly.
    pub confidence: String,
}

/// `adr:mutate_flow(id, patch)` request body.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MutateFlowPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_hunks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_hunks: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub add_entities: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub remove_entities: Vec<String>,
}

/// `adr:finalize()` result. Either the accepted flow list or a reject
/// reason pointing at the invariant that failed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "lowercase")]
pub enum FinalizeOutcome {
    Accepted {
        flows: Vec<adr_core::Flow>,
    },
    Rejected {
        rejected_rule: String,
        detail: String,
    },
}
