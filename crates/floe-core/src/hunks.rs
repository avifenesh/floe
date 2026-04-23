use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::graph::{EdgeId, NodeId};
use crate::provenance::Provenance;

/// The three semantic hunk types scope 1 delivers. The RFC lists more (lock, data,
/// docs, deletion); they land in later scopes without a schema bump — new variants
/// are additive.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HunkKind {
    /// A call-graph edge appeared, disappeared, or moved.
    Call {
        added_edges: Vec<EdgeId>,
        removed_edges: Vec<EdgeId>,
    },
    /// A string-union state gained or lost variants, or transitions changed.
    State {
        node: NodeId,
        added_variants: Vec<String>,
        removed_variants: Vec<String>,
    },
    /// An exported API surface changed shape or a route handler moved.
    Api {
        node: NodeId,
        before_signature: Option<String>,
        after_signature: Option<String>,
    },
    /// Data-shape change on a serde-serializable struct, Zod schema, or
    /// event-payload interface. `added` / `removed` / `renamed` describe
    /// field-level deltas so reviewers see schema drift at a glance.
    /// `type_name` is the (file-local) identifier of the struct /
    /// `z.object(...)` / interface.
    Data {
        file: String,
        type_name: String,
        added_fields: Vec<String>,
        removed_fields: Vec<String>,
        /// Pairs of `(before, after)` names for a rename heuristic —
        /// same field count, same type, different name.
        renamed_fields: Vec<(String, String)>,
    },
    /// Entity (Function / Type / State) present in base but absent in
    /// head, with no remaining references in head — dead-code removal.
    /// `was_exported` is true if the base declaration carried an
    /// `export` / `pub` marker; exported deletions are higher-weight.
    Deletion {
        file: String,
        entity_name: String,
        was_exported: bool,
    },
    /// Docstring drifted from the code it documents. `drift_kind`
    /// describes the shape of the drift:
    ///   - `"param-count"`  doc lists N params, signature has M ≠ N
    ///   - `"param-names"`  doc lists wrong names (set mismatch)
    ///   - `"missing"`     signature exists but doc comment removed
    /// `target` is the (file, function/method qualified name).
    Docs {
        file: String,
        target: String,
        drift_kind: String,
    },
    /// Synchronization primitive appeared, disappeared, or changed class.
    /// Detected by source-scan — TS: `async-mutex`, `p-limit`, `p-queue`,
    /// `async-lock`, `Atomics`; Rust: `Mutex`, `RwLock`, `Atomic*`,
    /// `OnceCell`. `before`/`after` are primitive class names; `None`
    /// means the primitive wasn't present on that side.
    Lock {
        file: String,
        primitive: String,
        before: Option<String>,
        after: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
pub struct Hunk {
    pub id: String,
    pub kind: HunkKind,
    pub provenance: Provenance,
}
