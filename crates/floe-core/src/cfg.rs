//! Per-function control-flow graphs.
//!
//! Nodes are one of `Seq` · `Branch` · `Loop` · `AsyncBoundary` · `Throw` · `Try`
//! · `Return` · `Entry` · `Exit`. Edges are unlabeled transfers. CFGs are *not*
//! SSA; they're cheap structural approximations good enough for the
//! navigation-cost drivers. Async/await produce explicit `AsyncBoundary` yield
//! nodes so the frontend flow view can render them as packet hops.
//!
//! One [`Cfg`] is attached per `Function` node via [`CfgMap`].

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::graph::{NodeId, Span};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct CfgNodeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum CfgNodeKind {
    Entry,
    Exit,
    /// A straight-line sequence of statements; collapsed into one node for
    /// navigation purposes (we don't care about per-statement granularity at
    /// the CFG level — the source view keeps that).
    Seq,
    Branch,
    Loop,
    /// An `await` expression — a yield point that the flow view draws as a
    /// packet hop. `Throw` + `Try` capture exception edges.
    AsyncBoundary,
    Throw,
    Try,
    Return,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct CfgNode {
    pub id: CfgNodeId,
    pub kind: CfgNodeKind,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct CfgEdge {
    pub from: CfgNodeId,
    pub to: CfgNodeId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cfg {
    pub nodes: Vec<CfgNode>,
    pub edges: Vec<CfgEdge>,
}

/// Per-function CFG binding. Keyed by the Function's `NodeId` in the enclosing
/// [`crate::graph::Graph`]. Stored as a flat list (not a map) because JSON
/// doesn't allow non-string map keys, and a wire-format map would waste tokens
/// when the NodeId is already a u32.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct CfgEntry {
    pub function: NodeId,
    pub cfg: Cfg,
}

pub type CfgMap = Vec<CfgEntry>;
