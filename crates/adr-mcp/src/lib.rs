//! Host for the `@adr/pi-extension` tool contract.
//!
//! Implements the state + validation logic the LLM's tool calls hit.
//! This crate intentionally ships **no transport** — it exposes a [`Session`]
//! type plus the eight handlers from the contract as plain Rust functions.
//! A downstream binary (planned: `adr-mcp` MCP-over-stdio server in scope 3
//! phase B) wraps these in a JSON-RPC loop.
//!
//! Contract source of truth: `docs/adr-pi-extension.md` at repo root.
//!
//! ## Shape at a glance
//!
//! ```ignore
//! use adr_core::Artifact;
//! use adr_mcp::Session;
//!
//! let mut s = Session::new(artifact)?;
//!
//! // Read tools are pure queries against the seed artifact.
//! let hunks = s.list_hunks();
//! let initial = s.list_flows_initial();
//!
//! // Mutations build up the working flow list.
//! let f1 = s.propose_flow("budget widening", "rationale…", vec!["hunk-1".into()], vec![])?;
//! let _f2 = s.propose_flow("streaming add", "rationale…", vec!["hunk-2".into()], vec![])?;
//!
//! // Finalize runs invariants and returns the accepted list or a reject reason.
//! let outcome = s.finalize("gemma4:26b", "0.3.0")?;
//! ```

pub mod errors;
pub mod fs_tools;
pub mod handlers;
pub mod invariants;
pub mod state;
pub mod wire;

pub use errors::{ErrorCode, ToolError};
pub use state::Session;
pub use wire::{
    EntityDescriptor, FinalizeOutcome, FlowInitial, HunkSummary, MutateFlowPatch, NeighborEdge,
    NeighborsResponse, Side,
};
