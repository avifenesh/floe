use std::collections::HashMap;

use floe_core::graph::{Graph, Node, NodeId, NodeKind};
use floe_core::hunks::{Hunk, HunkKind};
use floe_core::provenance::Provenance;

const SOURCE: &str = "floe-hunks/state";
const VERSION: &str = "0.1.0";

/// Emit one `State` hunk per state-machine whose variant set differs between
/// base and head. Identity is `(file, state_name)`. Added/removed refer to
/// variant-level changes only; full rename detection is out of scope for v0.
///
/// The `node` payload is the head-side `NodeId` when the state still exists in
/// head, otherwise the base-side id. Graph-local ids are opaque to downstream
/// consumers — they join back via (file, name) from the referenced node.
pub fn extract_state_hunks(base: &Graph, head: &Graph) -> Vec<Hunk> {
    let base_states = collect_states(base);
    let head_states = collect_states(head);

    let mut out = Vec::new();
    for (key, head_state) in &head_states {
        match base_states.get(key) {
            Some(base_state) => {
                let added: Vec<String> = head_state
                    .variants
                    .iter()
                    .filter(|v| !base_state.variants.contains(v))
                    .cloned()
                    .collect();
                let removed: Vec<String> = base_state
                    .variants
                    .iter()
                    .filter(|v| !head_state.variants.contains(v))
                    .cloned()
                    .collect();
                if added.is_empty() && removed.is_empty() {
                    continue;
                }
                out.push(build_hunk(head_state.node_id, added, removed, key));
            }
            None => {
                // entirely new state machine
                out.push(build_hunk(
                    head_state.node_id,
                    head_state.variants.clone(),
                    Vec::new(),
                    key,
                ));
            }
        }
    }
    for (key, base_state) in &base_states {
        if head_states.contains_key(key) {
            continue;
        }
        // state machine removed in head
        out.push(build_hunk(
            base_state.node_id,
            Vec::new(),
            base_state.variants.clone(),
            key,
        ));
    }
    out
}

fn build_hunk(
    node: NodeId,
    added: Vec<String>,
    removed: Vec<String>,
    key: &(String, String),
) -> Hunk {
    let id_payload = serde_json::to_vec(&(&key.0, &key.1, &added, &removed)).unwrap_or_default();
    Hunk {
        id: format!("state-{}", blake3::hash(&id_payload).to_hex()),
        kind: HunkKind::State {
            node,
            added_variants: added,
            removed_variants: removed,
        },
        provenance: Provenance::new(SOURCE, VERSION, "hunks", &id_payload),
    }
}

struct StateRef {
    node_id: NodeId,
    variants: Vec<String>,
}

fn collect_states(g: &Graph) -> HashMap<(String, String), StateRef> {
    let mut map = HashMap::new();
    for n in &g.nodes {
        if let Node {
            id,
            kind: NodeKind::State { name, variants },
            file,
            ..
        } = n
        {
            map.insert(
                (file.clone(), name.clone()),
                StateRef {
                    node_id: *id,
                    variants: variants.clone(),
                },
            );
        }
    }
    map
}
