//! Graph schema v0.1 — the machine-readable substrate every downstream crate reads.
//!
//! Schema versioning lives in [`Artifact::schema_version`]. Bump the minor on breaking
//! changes until v1. Every node and edge carries [`Provenance`] so downstream views can
//! explain where a fact came from.

pub mod graph;
pub mod provenance;
pub mod hunks;
pub mod cfg;
pub mod artifact;

pub use artifact::{Artifact, SCHEMA_VERSION};
pub use cfg::{Cfg, CfgEdge, CfgEntry, CfgMap, CfgNode, CfgNodeId, CfgNodeKind};
pub use graph::{Edge, EdgeId, EdgeKind, Graph, Node, NodeId, NodeKind};
pub use hunks::{Hunk, HunkKind};
pub use provenance::Provenance;
