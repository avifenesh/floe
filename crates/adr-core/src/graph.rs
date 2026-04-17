use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::provenance::Provenance;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct NodeId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, JsonSchema)]
pub struct EdgeId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum NodeKind {
    /// A TypeScript function, method, or arrow-function definition.
    Function { name: String, signature: String },
    /// A type alias, interface, or class declared in the surface.
    Type { name: String },
    /// A string-union state (e.g. `type State = "a" | "b"`).
    State { name: String, variants: Vec<String> },
    /// An HTTP / RPC endpoint exposed by the repo (Next.js handler, etc.).
    ApiEndpoint { method: String, path: String },
    /// A source file referenced by other nodes.
    File { path: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum EdgeKind {
    Calls,
    Defines,
    Exports,
    /// A transition between two [`NodeKind::State`] variants.
    Transitions { from: String, to: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Node {
    pub id: NodeId,
    pub kind: NodeKind,
    pub file: String,
    pub span: Span,
    pub provenance: Provenance,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Edge {
    pub id: EdgeId,
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    pub provenance: Provenance,
}

/// Byte-range span in the indicated file. Line/column are derived on demand by the
/// renderer; the artifact stays source-of-truth in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Span {
    pub start: u32,
    pub end: u32,
}

/// Flat adjacency-list graph. `petgraph` is used internally for queries; the wire
/// format stays flat so downstream languages can read the artifact without a graph
/// library.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Graph {
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
}

impl Graph {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn edges_from(&self, id: NodeId) -> impl Iterator<Item = &Edge> {
        self.edges.iter().filter(move |e| e.from == id)
    }
}
