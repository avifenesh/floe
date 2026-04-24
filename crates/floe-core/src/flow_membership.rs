//! LLM-curated flow membership — the model's narrative of which
//! entities actually participate in a flow's story plus the shape of
//! the call/data relationships between them.
//!
//! Produced by a per-flow GLM-4.7 session (see
//! `floe-server/src/llm/flow_membership.rs`). Rendered by the Flow
//! workspace as the primary graph when present; the deterministic
//! BFS propagation stays as the fallback floor.
//!
//! # Contract with the model
//!
//! The shape below matches the JSON the model returns verbatim, so
//! tightening the schema here tightens the prompt contract. Keep
//! fields additive — older cached artifacts must still deserialise.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema, Default)]
pub struct FlowMembership {
    /// Entities the model considers first-class participants.
    /// Capped at ~10 by the prompt; UI renders these as full nodes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<MembershipMember>,
    /// Coarse groupings that collapse periphery (test scaffolding,
    /// incidental helpers, peripheral types). UI renders as
    /// collapsible chips.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary_groups: Vec<MembershipGroup>,
    /// Edges between members. `kind` is a free-form string (`call`,
    /// `data-flow`, `transition`, etc.) so the model can express
    /// relationships beyond our hunk kinds.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edges: Vec<MembershipEdge>,
    /// Structural shapes (loops, branches, fan-outs) the model
    /// declared. Richer than flat edges; UI draws dedicated overlays.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<MembershipShape>,
    /// Stamp so the UI can show which model curated this view.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub model: String,
    /// LLM-emitted flow diagrams. Typically two entries: `head`
    /// (current state) and, when base differs meaningfully, `base`.
    /// Head should use mermaid `classDef` to mark added / removed /
    /// unchanged nodes so the diff is visible without a second
    /// picture. Rendered by Mermaid.js in the UI.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagrams: Vec<MembershipDiagram>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MembershipDiagram {
    /// Diagram notation — currently only `"mermaid"`; kept string-
    /// typed so the schema can widen (DOT, PlantUML) without a
    /// breaking bump.
    pub kind: String,
    /// Reviewer-facing label — typically `"head"` / `"base"` /
    /// `"diff"`, but free-form so the model can label specialised
    /// views (e.g. `"retry loop only"`).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub label: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MembershipMember {
    pub entity: String,
    /// Free-form: typically `core` / `entrance` / `exit`.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    /// Free-form label for the model's grouping (`caller`, `flow
    /// center`, `transport`, etc.). Not machine-interpretable.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub side: String,
    /// One-sentence justification from the model.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub why: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MembershipGroup {
    pub label: String,
    #[serde(default)]
    pub count: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sample_entities: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MembershipEdge {
    pub from: String,
    pub to: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
}

/// `kind` drives which optional fields matter:
/// - `loop`:   `nodes` cycle
/// - `branch`: `at` + `paths` (each path is an ordered token list)
/// - `fanout`: `from` + `to`
///
/// Shapes are allowed to reference tokens that aren't entity names
/// (e.g. `"return"`, `"continue"`) so the model can model control-flow
/// outcomes; the renderer treats those as labels, not node refs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct MembershipShape {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub nodes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub at: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub paths: Vec<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub to: Vec<String>,
}
