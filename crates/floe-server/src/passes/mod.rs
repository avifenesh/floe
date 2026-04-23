//! Worker-time passes that run alongside (or after) the
//! structural/LLM pipeline to enrich the artifact with
//! primary-source evidence. Each pass is gated on an env flag so
//! individual passes can be disabled without rebuilding.
//!
//! Currently shipped:
//! - [`compile`] — `tsc --noEmit` delta on base vs head, emits
//!   per-flow [`CompileDiagnostic`] claims (RFC Appendix F upgrade #3).
//!
//! [`CompileDiagnostic`]: floe_core::CompileDiagnostic

pub mod claim_anchors;
pub mod compile;
pub mod coverage;
pub mod external_runs;
pub mod intent_extract;
