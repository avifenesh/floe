//! Reviewer notes anchored to specific objects in the artifact —
//! hunks, flows, entities, intent claims, or base/head file lines.
//!
//! The model is GitHub line-comments on a diff, generalised: a note
//! carries a discriminated `anchor` so downstream consumers (and the
//! `/notes/export` endpoint) can bundle each note with its object's
//! context when handing work off to a coding agent.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Which object an inline note sticks to. The enum variants carry
/// just the identifiers needed to locate the object inside the
/// artifact; the export pass rehydrates surrounding context (code
/// snippet, flow name, claim text) on read.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum InlineNoteAnchor {
    /// Anchored to a hunk by id.
    Hunk { hunk_id: String },
    /// Anchored to a flow by id.
    Flow { flow_id: String },
    /// Anchored to an entity by its qualified name (as seen in the
    /// graph — `ClassName.methodName` for class methods, bare name
    /// for free functions).
    Entity { entity_name: String },
    /// Anchored to a single claim inside `artifact.intent.claims`.
    IntentClaim { claim_index: usize },
    /// Anchored to a line in a source file, either side of the diff.
    FileLine {
        file: String,
        #[serde(rename = "line_side")]
        side: FileLineSide,
        line: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum FileLineSide {
    Base,
    Head,
}

/// A reviewer note attached to one object in the artifact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct InlineNote {
    /// `note-<blake3>` — stable across requests, derived from
    /// `(anchor, text, created_at)`.
    pub id: String,
    pub anchor: InlineNoteAnchor,
    pub text: String,
    /// Opaque user identifier — GitHub login when OAuth is on, or
    /// `"local"` in dev. Not validated against the current session.
    pub author: String,
    /// RFC3339 timestamp, stamped by the server on insert.
    pub created_at: String,
}

impl InlineNote {
    pub fn derive_id(
        anchor: &InlineNoteAnchor,
        text: &str,
        created_at: &str,
    ) -> String {
        let payload = serde_json::to_string(anchor).unwrap_or_default();
        let mut h = blake3::Hasher::new();
        h.update(b"inline-note-v1|");
        h.update(payload.as_bytes());
        h.update(b"|");
        h.update(text.as_bytes());
        h.update(b"|");
        h.update(created_at.as_bytes());
        format!("note-{}", h.finalize().to_hex())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_is_deterministic() {
        let a = InlineNoteAnchor::Flow {
            flow_id: "flow-1".into(),
        };
        let id1 = InlineNote::derive_id(&a, "hi", "2026-04-23T00:00:00Z");
        let id2 = InlineNote::derive_id(&a, "hi", "2026-04-23T00:00:00Z");
        assert_eq!(id1, id2);
        assert!(id1.starts_with("note-"));
    }

    #[test]
    fn id_changes_with_anchor() {
        let a = InlineNoteAnchor::Flow { flow_id: "flow-1".into() };
        let b = InlineNoteAnchor::Flow { flow_id: "flow-2".into() };
        let t = "2026-04-23T00:00:00Z";
        assert_ne!(
            InlineNote::derive_id(&a, "hi", t),
            InlineNote::derive_id(&b, "hi", t)
        );
    }

    #[test]
    fn anchor_serialises_with_kind_tag() {
        let a = InlineNoteAnchor::FileLine {
            file: "src/x.ts".into(),
            side: FileLineSide::Head,
            line: 42,
        };
        let s = serde_json::to_string(&a).unwrap();
        assert!(s.contains("\"kind\":\"file-line\""));
        assert!(s.contains("\"line_side\":\"head\""));
    }
}
