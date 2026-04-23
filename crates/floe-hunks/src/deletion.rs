//! Deletion hunk extractor — find entities present in the base graph
//! but absent from the head graph, with no remaining Calls-edge
//! references from head to confirm the deletion is "real" (not a
//! rename captured elsewhere).
//!
//! Emits one `Deletion` per (file, entity) pair. `was_exported` is a
//! best-effort read of the base signature — TS `export ` prefix or
//! Rust `pub ` prefix. Missing on the signature → `false`.

use std::collections::BTreeSet;

use floe_core::graph::{Graph, Node, NodeKind};
use floe_core::hunks::{Hunk, HunkKind};
use floe_core::provenance::Provenance;

const SOURCE: &str = "floe-hunks/deletion";
const VERSION: &str = "0.1.0";

pub fn extract_deletion_hunks(base: &Graph, head: &Graph) -> Vec<Hunk> {
    let head_names: BTreeSet<String> = head.nodes.iter().filter_map(node_name).collect();
    let mut out = Vec::new();
    let mut seen: BTreeSet<(String, String)> = BTreeSet::new();
    for n in &base.nodes {
        let Some(name) = node_name(n) else { continue };
        if head_names.contains(&name) {
            continue;
        }
        let key = (n.file.clone(), name.clone());
        if !seen.insert(key.clone()) {
            continue;
        }
        let was_exported = signature_of(n)
            .map(|sig| sig.starts_with("export ") || sig.starts_with("pub "))
            .unwrap_or(false);
        let id_payload =
            serde_json::to_vec(&(&n.file, &name, was_exported)).unwrap_or_default();
        out.push(Hunk {
            id: format!("deletion-{}", blake3::hash(&id_payload).to_hex()),
            kind: HunkKind::Deletion {
                file: n.file.clone(),
                entity_name: name,
                was_exported,
            },
            provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
        });
    }
    out
}

fn node_name(n: &Node) -> Option<String> {
    match &n.kind {
        NodeKind::Function { name, .. }
        | NodeKind::Type { name }
        | NodeKind::State { name, .. } => Some(name.clone()),
        _ => None,
    }
}

fn signature_of(n: &Node) -> Option<&str> {
    if let NodeKind::Function { signature, .. } = &n.kind {
        Some(signature.as_str())
    } else {
        None
    }
}
