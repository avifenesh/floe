use std::collections::{HashMap, HashSet};

use adr_core::graph::{EdgeKind, Graph, Node, NodeId, NodeKind};
use adr_core::hunks::{Hunk, HunkKind};
use adr_core::provenance::Provenance;

const SOURCE: &str = "adr-hunks/api";
const VERSION: &str = "0.1.0";

/// Emit one `Api` hunk per exported-function surface whose signature differs
/// between base and head, or that was added/removed. Identity is `(file, name)`.
///
/// Exported-type shape diffs are tracked structurally via call edges + future
/// hunks; this extractor covers the function-signature surface only for v0.
pub fn extract_api_hunks(base: &Graph, head: &Graph) -> Vec<Hunk> {
    let base_api = collect_exported_fns(base);
    let head_api = collect_exported_fns(head);

    let mut seen: HashSet<&(String, String)> = HashSet::new();
    let mut out = Vec::new();
    for key in base_api.keys().chain(head_api.keys()) {
        if !seen.insert(key) {
            continue;
        }
        let b = base_api.get(key);
        let h = head_api.get(key);
        match (b, h) {
            (Some(be), Some(he)) if be.signature == he.signature => {}
            (Some(be), Some(he)) => out.push(build_hunk(
                he.node_id,
                Some(be.signature.clone()),
                Some(he.signature.clone()),
                key,
            )),
            (None, Some(he)) => out.push(build_hunk(
                he.node_id,
                None,
                Some(he.signature.clone()),
                key,
            )),
            (Some(be), None) => out.push(build_hunk(
                be.node_id,
                Some(be.signature.clone()),
                None,
                key,
            )),
            (None, None) => {}
        }
    }
    out
}

fn build_hunk(
    node: NodeId,
    before: Option<String>,
    after: Option<String>,
    key: &(String, String),
) -> Hunk {
    let id_payload = serde_json::to_vec(&(&key.0, &key.1, &before, &after)).unwrap_or_default();
    Hunk {
        id: format!("api-{}", blake3::hash(&id_payload).to_hex()),
        kind: HunkKind::Api {
            node,
            before_signature: before,
            after_signature: after,
        },
        provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
    }
}

struct FnExport {
    node_id: NodeId,
    signature: String,
}

fn collect_exported_fns(g: &Graph) -> HashMap<(String, String), FnExport> {
    let exported: HashSet<NodeId> = g
        .edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Exports))
        .map(|e| e.to)
        .collect();
    let mut out = HashMap::new();
    for n in &g.nodes {
        if !exported.contains(&n.id) {
            continue;
        }
        if let Node {
            id,
            kind: NodeKind::Function { name, signature },
            file,
            ..
        } = n
        {
            out.insert(
                (file.clone(), name.clone()),
                FnExport {
                    node_id: *id,
                    signature: signature.clone(),
                },
            );
        }
    }
    out
}
