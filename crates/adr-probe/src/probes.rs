//! The frozen probe question set for v0.1.
//!
//! Three questions, each runs in its own clean session. Any change to
//! the questions, the system prompt wrapper, or the weight constants in
//! [`crate::aggregate`] bumps [`PROBE_SET_VERSION`] and invalidates
//! every baseline computed under the old version.

use serde::{Deserialize, Serialize};

/// Version tag baked into every baseline. Any edit to the probes or to
/// the measurement weights bumps this.
///
/// v0.2 (2026-04-18): dropped `max_turns` from 40 to 12. Sessions that
/// hit 40 turns burned 3M input tokens each on glide-mq #181 and
/// rarely improved visit coverage past turn 12 — the model was
/// spinning, not discovering. Old baselines are invalidated to avoid
/// mixing the two regimes in the same per-axis total.
pub const PROBE_SET_VERSION: &str = "0.2";

/// Stable id for each probe. Used as the filename in the baseline
/// directory and as the key in aggregated results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProbeId {
    ApiSurface,
    ExternalBoundaries,
    TypeCallsites,
}

impl ProbeId {
    pub fn as_str(self) -> &'static str {
        match self {
            ProbeId::ApiSurface => "probe-api-surface",
            ProbeId::ExternalBoundaries => "probe-external-boundaries",
            ProbeId::TypeCallsites => "probe-type-callsites",
        }
    }
}

/// A single probe definition — frozen prompt + metadata.
#[derive(Debug, Clone)]
pub struct ProbeDef {
    pub id: ProbeId,
    /// One-line human description for logs / UI.
    pub label: &'static str,
    /// The exact question the model answers. Kept identical every run
    /// for cross-PR comparability.
    pub question: &'static str,
    /// System-prompt wrapper. Tells the model it's answering for
    /// measurement purposes, not for human consumption — keeps it brief
    /// and tool-driven.
    pub system_prompt: &'static str,
    /// Hard cap on turns per session. Runaway loops are a budget leak
    /// and a sign the probe isn't terminating.
    ///
    /// v0.1 calibration (2026-04-18): 40-turn cap regularly hit
    /// `max-turns` end-reason on big repos (3M input tokens per probe
    /// on glide-mq #181). Lowered to 12 — the navigation question
    /// doesn't need more than ~10 tool calls to answer. If the model
    /// needs more than that it's spinning, not discovering. Session
    /// still exits early on empty `tool_calls[]` (final answer).
    pub max_turns: u32,
}

/// The frozen v0.1 probe set. Callers enumerate this and run each probe
/// in a clean session.
pub fn probe_set() -> [ProbeDef; 3] {
    [
        ProbeDef {
            id: ProbeId::ApiSurface,
            label: "public API surface",
            question:
                "Map the public API of this repository. For every exported function, \
                 class, or method, emit one line: `<qualified name>: <one-sentence description \
                 of functionality>`. Use adr.list_hunks / adr.get_entity / adr.neighbors and \
                 the built-in read / grep / glob tools to inspect. End with a one-line summary \
                 of how many entities you mapped.",
            system_prompt: SYSTEM_PROMPT,
            max_turns: 12,
        },
        ProbeDef {
            id: ProbeId::ExternalBoundaries,
            label: "external boundaries",
            question:
                "List every external boundary the repository crosses: network calls (fetch, \
                 axios, ws, redis, db client, grpc), filesystem writes, subprocess spawns, env \
                 reads, and any I/O that leaves the process. For each, emit: `<file>:<line> \
                 <kind> <target>`. Flag trust classifications when obvious (user-input-sink, \
                 internal-service, etc.). End with a one-line summary of count per kind.",
            system_prompt: SYSTEM_PROMPT,
            max_turns: 12,
        },
        ProbeDef {
            id: ProbeId::TypeCallsites,
            label: "type call-sites",
            question:
                "For every exported type (interface, type alias, class, enum), list every \
                 call-site that constructs, extends, implements, or destructures it. Emit: \
                 `<type>: <file>:<line> <usage-kind>`. End with a one-line summary of total \
                 type-usage pairs.",
            system_prompt: SYSTEM_PROMPT,
            max_turns: 12,
        },
    ]
}

const SYSTEM_PROMPT: &str = "\
You are a code-navigation probe. Your task is not to produce human-friendly \
prose; your task is to visit the relevant code and emit the requested structured \
listing. We are measuring the navigation effort itself, not the eloquence of your \
answer. Therefore:\n\
\n\
- Call tools aggressively; don't guess from names when a read or neighbors call \
  would confirm.\n\
- Keep the final answer compact — one line per item — no explanations.\n\
- When a file's worth of content is unnecessary for the answer, prefer `grep` over \
  `read`.\n\
- Do not invent symbols. If a symbol isn't visible via the tools, omit it and note \
  the omission in the final summary.\n\
- When you have enough to answer, emit the answer and stop. Extra turns after the \
  answer is ready are wasted budget.\n";
