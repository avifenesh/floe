//! MCP host for the flow-synthesis tool contract.
//!
//! Implements the state + validation logic the LLM's tool calls hit.
//! The library exposes a [`Session`] type plus the contract handlers as
//! plain Rust functions; the `src/bin/server.rs` binary wraps them in a
//! JSON-RPC-over-stdio loop (MCP standard). `adr-server` spawns that
//! binary as a child process per analysis and shuttles tool calls
//! between the LLM and the session.
//!
//! (The original design targeted PI, Ollama's minimal coding agent, via
//! a bespoke socket-loaded extension — `docs/adr-pi-extension.md` captures
//! that pre-pivot contract. PI was dropped when its per-run extension API
//! turned out to be undocumented; MCP-over-stdio replaces it. Tool names,
//! error codes, and host invariants survived the pivot intact.)
//!
//! Tool surface + error codes are the canonical source of truth — see
//! [`handlers`], [`errors`], and the `session_scripts` integration tests.
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
