//! LLM flow-synthesis integration — talks to an Ollama model through the
//! `floe-mcp` child process, runs the agent loop, and returns the final
//! flow list (or `None` if the LLM rejected, errored, or is not
//! configured).
//!
//! Wiring:
//!
//! ```text
//!   floe-server (this module)
//!        │
//!        │   JSON-RPC over stdio
//!        ├──────────────▶ floe-mcp child  ──────▶ artifact state + invariants
//!        │
//!        │   HTTP /api/chat
//!        └──────────────▶ Ollama          ──────▶ Gemma 4 26B (or other)
//! ```
//!
//! The server is both the MCP client and the Ollama client; Ollama's
//! tool-call responses get forwarded to `floe-mcp`, and results flow back
//! through the server as role=tool messages to Ollama. Loop ends when the
//! model calls `floe.finalize`.

pub mod config;
pub mod flow_membership;
pub mod glm_client;
pub mod intent_pipeline;
pub mod mcp_client;
pub mod model_defaults;
pub mod ollama_client;
pub mod probe_client;
pub mod probe_pipeline;
pub mod prompt;
pub mod summary_pass;
pub mod synth_parallel;
pub mod synthesize;
pub mod tool_call_drift;

pub use config::{LlmConfig, LlmProvider};
pub use synthesize::{synthesize, SynthesisOutcome};
