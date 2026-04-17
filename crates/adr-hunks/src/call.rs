use std::collections::HashSet;

use adr_core::graph::{Edge, EdgeKind, Graph, Node, NodeKind};
use adr_core::hunks::{Hunk, HunkKind};
use adr_core::provenance::Provenance;

const SOURCE: &str = "adr-hunks/call";
const VERSION: &str = "0.1.0";

/// Emit a single `Call` hunk covering every `Calls` edge that is in head but
/// not base, and vice versa. Returns `None` if both graphs project to the
/// same set of call edges.
pub fn extract_call_hunk(base: &Graph, head: &Graph) -> Option<Hunk> {
    let base_calls: Vec<CallRef> = project_calls(base);
    let head_calls: Vec<CallRef> = project_calls(head);

    let base_keys: HashSet<&CallKey> = base_calls.iter().map(|c| &c.key).collect();
    let head_keys: HashSet<&CallKey> = head_calls.iter().map(|c| &c.key).collect();

    let added: Vec<_> = head_calls
        .iter()
        .filter(|c| !base_keys.contains(&c.key))
        .map(|c| c.edge_id)
        .collect();
    let removed: Vec<_> = base_calls
        .iter()
        .filter(|c| !head_keys.contains(&c.key))
        .map(|c| c.edge_id)
        .collect();

    if added.is_empty() && removed.is_empty() {
        return None;
    }

    let id_payload = serde_json::to_vec(&(added.len(), removed.len())).unwrap_or_default();
    Some(Hunk {
        id: format!("call-{}", blake3::hash(&id_payload).to_hex()),
        kind: HunkKind::Call {
            added_edges: added,
            removed_edges: removed,
        },
        provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
    })
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct CallKey {
    caller_file: String,
    caller_name: String,
    callee_name: String,
}

struct CallRef {
    key: CallKey,
    edge_id: adr_core::graph::EdgeId,
}

fn project_calls(g: &Graph) -> Vec<CallRef> {
    g.edges
        .iter()
        .filter(|e| matches!(e.kind, EdgeKind::Calls))
        .filter_map(|e| call_ref(g, e))
        .collect()
}

fn call_ref(g: &Graph, edge: &Edge) -> Option<CallRef> {
    let caller = g.node(edge.from)?;
    let callee = g.node(edge.to)?;
    let caller_name = function_name(caller)?;
    let callee_name = function_name(callee)?;
    Some(CallRef {
        key: CallKey {
            caller_file: caller.file.clone(),
            caller_name: caller_name.to_string(),
            callee_name: callee_name.to_string(),
        },
        edge_id: edge.id,
    })
}

fn function_name(n: &Node) -> Option<&str> {
    match &n.kind {
        NodeKind::Function { name, .. } => Some(name.as_str()),
        _ => None,
    }
}
