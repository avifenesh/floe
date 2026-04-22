//! LLM-navigation probes. Measures how expensive it is for a pinned LLM
//! to answer a fixed set of questions about the repo. The observed
//! effort (tokens, tool calls, turns) becomes the per-entity cost
//! baseline that feeds the per-flow signed cost delta.
//!
//! See `docs/scope-5-cost-model.md` for the end-to-end design.
//!
//! Crate structure:
//!
//! - [`probes`] — the frozen question set + metadata.
//! - [`session`] — [`ProbeSession`] runs one probe against a
//!   caller-supplied [`ProbeClient`], measuring along the way.
//! - [`aggregate`] — fold per-session observations into a single
//!   `per-entity` cost map across all three probes.
//! - [`storage`] — baseline read / write / invalidate on local
//!   filesystem (S3 backing lands later).
//!
//! The crate owns **no** LLM transport. Callers (today: `adr-server`)
//! wire their existing Ollama / GLM clients through the [`ProbeClient`]
//! trait. That keeps this crate testable without a real model.

pub mod aggregate;
pub mod probes;
pub mod session;
pub mod storage;

pub use aggregate::{aggregate, AggregateBaseline, EntityCost};
pub use probes::{probe_set, ProbeDef, ProbeId, PROBE_SET_VERSION};
pub use session::{
    ChatReply, Msg, ProbeClient, ProbeResult, ProbeSession, ToolDef, ToolDispatchResult,
};
pub use storage::{BaselineKey, BaselineStore, BaselineStatus};
