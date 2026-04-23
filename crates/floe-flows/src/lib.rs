//! Structural flow clustering — the deterministic floor.
//!
//! A flow here is the smallest useful group of hunks that "probably belong
//! together". We group by shared qualified-name prefix (class or top-level
//! bucket). This is intentionally sloppy — it catches the common case
//! (all `Queue.*` method changes end up together) and lets the LLM refine
//! via the LLM-synthesis pass (floe-mcp tool contract). When LLM synthesis is off or rejected, the
//! UI renders these clusters with a visible "structural only" banner.

use std::collections::BTreeMap;

use floe_core::flow::{Flow, FlowSource};
use floe_core::graph::{Graph, Node, NodeKind};
use floe_core::hunks::{Hunk, HunkKind};
use floe_core::Artifact;

/// Entry point. Walks the artifact's hunks, groups them by shared qualified
/// prefix, emits a stable, deterministic flow list.
pub fn cluster(artifact: &Artifact) -> Vec<Flow> {
    let mut by_prefix: BTreeMap<String, Vec<HunkRef>> = BTreeMap::new();
    for h in &artifact.hunks {
        let entities = hunk_entities(h, artifact);
        let bucket = classify_bucket(&entities);
        by_prefix.entry(bucket).or_default().push(HunkRef {
            hunk_id: h.id.clone(),
            entities,
        });
    }
    let mut flows: Vec<Flow> = by_prefix
        .into_iter()
        .enumerate()
        .map(|(i, (bucket, refs))| make_flow(i, &bucket, &refs))
        .collect();
    // Fill propagation_edges: 1-hop callers/callees that touch a flow's
    // entities but aren't themselves in the flow. Gives the reviewer
    // "who else reaches into this" context without the full graph.
    populate_propagation(&mut flows, artifact);
    flows
}

/// For each flow, scan head graph edges and keep the ones where one
/// endpoint is a flow entity and the other isn't — those are 1-hop
/// propagation boundaries.
/// Max call-graph hops to walk outward from flow entities when
/// computing propagation context. Reviewers want "entrance → flow →
/// end" end-to-end, not just the immediate caller / callee; but
/// unbounded BFS over a large repo floods the graph with unrelated
/// code. Three hops covers the common component shape (request
/// handler → service → helper → leaf) without runaway growth.
const PROPAGATION_MAX_HOPS: u32 = 3;

fn populate_propagation(flows: &mut [Flow], artifact: &Artifact) {
    use floe_core::graph::EdgeKind;
    let id_to_qname = node_qname_map(&artifact.head);
    // Qualified name → node id map for upstream/downstream lookups.
    let qname_to_id: std::collections::HashMap<String, floe_core::graph::NodeId> =
        id_to_qname
            .iter()
            .map(|(id, name)| (name.clone(), *id))
            .collect();
    // Only Function/Type/State count as call-graph participants.
    // File nodes are containment; ApiEndpoint nodes are routing
    // metadata. Both would masquerade as callers otherwise.
    let is_callable = |id: floe_core::graph::NodeId| -> bool {
        artifact
            .head
            .nodes
            .iter()
            .find(|n| n.id == id)
            .map(|n| {
                matches!(
                    &n.kind,
                    NodeKind::Function { .. }
                        | NodeKind::Type { .. }
                        | NodeKind::State { .. }
                )
            })
            .unwrap_or(false)
    };
    // Index edges by endpoint for O(1) hop expansion. Only `Calls`
    // edges carry "A reaches into B" semantics.
    let mut out_edges: std::collections::HashMap<
        floe_core::graph::NodeId,
        Vec<floe_core::graph::NodeId>,
    > = std::collections::HashMap::new();
    let mut in_edges: std::collections::HashMap<
        floe_core::graph::NodeId,
        Vec<floe_core::graph::NodeId>,
    > = std::collections::HashMap::new();
    for e in &artifact.head.edges {
        if !matches!(e.kind, EdgeKind::Calls) {
            continue;
        }
        if !is_callable(e.from) || !is_callable(e.to) {
            continue;
        }
        out_edges.entry(e.from).or_default().push(e.to);
        in_edges.entry(e.to).or_default().push(e.from);
    }

    for flow in flows.iter_mut() {
        // Seed set: the flow's own entities, resolved to node ids.
        let seed_ids: std::collections::HashSet<floe_core::graph::NodeId> = flow
            .entities
            .iter()
            .filter_map(|q| qname_to_id.get(q).copied())
            .collect();
        // BFS outward — combined callers (in-edges) and callees
        // (out-edges). Track the full reach so we can emit every
        // edge along the chain, not just the hop-1 frontier.
        let mut reach: std::collections::HashSet<floe_core::graph::NodeId> =
            seed_ids.clone();
        let mut frontier = seed_ids.clone();
        for _hop in 0..PROPAGATION_MAX_HOPS {
            let mut next: std::collections::HashSet<floe_core::graph::NodeId> =
                std::collections::HashSet::new();
            for &id in &frontier {
                for &c in out_edges.get(&id).into_iter().flatten() {
                    if !reach.contains(&c) {
                        next.insert(c);
                    }
                }
                for &c in in_edges.get(&id).into_iter().flatten() {
                    if !reach.contains(&c) {
                        next.insert(c);
                    }
                }
            }
            if next.is_empty() {
                break;
            }
            reach.extend(next.iter().copied());
            frontier = next;
        }

        // Emit every Calls edge whose endpoints both lie in the
        // reach set, excluding edges where *both* endpoints are
        // seed entities (those are the flow's own internal shape,
        // already rendered by the graph itself).
        let mut seen: std::collections::HashSet<(String, String)> =
            std::collections::HashSet::new();
        for e in &artifact.head.edges {
            if !matches!(e.kind, EdgeKind::Calls) {
                continue;
            }
            if !reach.contains(&e.from) || !reach.contains(&e.to) {
                continue;
            }
            let from_is_seed = seed_ids.contains(&e.from);
            let to_is_seed = seed_ids.contains(&e.to);
            if from_is_seed && to_is_seed {
                continue;
            }
            let (Some(f), Some(t)) = (
                id_to_qname.get(&e.from).cloned(),
                id_to_qname.get(&e.to).cloned(),
            ) else {
                continue;
            };
            let key = (f, t);
            if seen.insert(key.clone()) {
                flow.propagation_edges.push(key);
            }
        }
    }
}

fn node_qname_map(g: &Graph) -> BTreeMap<floe_core::graph::NodeId, String> {
    let mut out = BTreeMap::new();
    for n in &g.nodes {
        if let Some(name) = qualified_name_of(n) {
            out.insert(n.id, name);
        }
    }
    out
}

fn qualified_name_of(n: &Node) -> Option<String> {
    match &n.kind {
        NodeKind::Function { name, .. } => Some(name.clone()),
        NodeKind::Type { name, .. } => Some(name.clone()),
        NodeKind::State { name, .. } => Some(name.clone()),
        NodeKind::ApiEndpoint { path, .. } => Some(path.clone()),
        NodeKind::File { path } => Some(path.clone()),
    }
}

struct HunkRef {
    hunk_id: String,
    entities: Vec<String>,
}

/// Pick the bucket key for a hunk given its entity names.
///
/// Heuristic: take the first qualified-name prefix (before the first `.`).
/// `Queue.setBudget` → `Queue`. `recordUsageAndCheckBudget` → `top-level`.
/// Multiple entities pick the first. It's a first pass — the LLM can merge
/// or split later.
fn classify_bucket(entities: &[String]) -> String {
    for e in entities {
        if let Some((prefix, _)) = e.split_once('.') {
            return prefix.to_string();
        }
    }
    "top-level".to_string()
}

fn make_flow(index: usize, bucket: &str, refs: &[HunkRef]) -> Flow {
    let hunk_ids: Vec<String> = refs.iter().map(|r| r.hunk_id.clone()).collect();
    let entities: Vec<String> = {
        let mut seen = std::collections::BTreeSet::new();
        for r in refs {
            for e in &r.entities {
                seen.insert(e.clone());
            }
        }
        seen.into_iter().collect()
    };
    let id_material = {
        let mut s = format!("{bucket}|");
        for h in &hunk_ids {
            s.push_str(h);
            s.push('|');
        }
        s
    };
    Flow {
        id: format!("flow-{}", blake3::hash(id_material.as_bytes()).to_hex()),
        name: format!("<structural: {bucket}>"),
        rationale: format!(
            "Shared qualified-name prefix: {bucket}. This is a structural cluster — \
             LLM synthesis can merge, split, or rename."
        ),
        source: FlowSource::Structural,
        hunk_ids,
        entities,
        extra_entities: Vec::new(),
        propagation_edges: Vec::new(),
        order: index as u32,
        evidence: Vec::new(),
        cost: None,
        intent_fit: None,
        proof: None,
    }
}

/// Resolve a hunk's referenced entities to qualified name strings. Same-name
/// resolution across base/head normalises `ClassName.methodName` identity so
/// hunks referencing the same symbol on either side share a bucket.
fn hunk_entities(hunk: &Hunk, artifact: &Artifact) -> Vec<String> {
    let mut out = Vec::new();
    match &hunk.kind {
        HunkKind::Call {
            added_edges,
            removed_edges,
        } => {
            for &id in added_edges {
                if let Some(e) = artifact.head.edges.iter().find(|x| x.id == id) {
                    collect_name(&artifact.head, e.from, &mut out);
                    collect_name(&artifact.head, e.to, &mut out);
                }
            }
            for &id in removed_edges {
                if let Some(e) = artifact.base.edges.iter().find(|x| x.id == id) {
                    collect_name(&artifact.base, e.from, &mut out);
                    collect_name(&artifact.base, e.to, &mut out);
                }
            }
        }
        HunkKind::State { node, .. } => {
            collect_name(&artifact.head, *node, &mut out);
            collect_name(&artifact.base, *node, &mut out);
        }
        HunkKind::Api { node, .. } => {
            collect_name(&artifact.head, *node, &mut out);
            collect_name(&artifact.base, *node, &mut out);
        }
        HunkKind::Lock { primitive, .. } => {
            // Bare primitive tail so class-prefix bucketing sees a
            // real identifier, not the file path. `async-mutex.Mutex`
            // → `Mutex`. Matches how Function/Type entities read.
            let tail = primitive.rsplit_once('.').map(|(_, t)| t).unwrap_or(primitive);
            out.push(tail.to_string());
        }
        HunkKind::Data { type_name, .. } => {
            out.push(type_name.clone());
        }
        HunkKind::Docs { target, .. } => {
            // `target` may be `ClassName.method` — keep as-is so the
            // bucket resolves to `ClassName` like a Function hunk.
            out.push(target.clone());
        }
        HunkKind::Deletion { entity_name, .. } => {
            out.push(entity_name.clone());
        }
    }
    out.sort();
    out.dedup();
    out
}

fn collect_name(g: &Graph, id: floe_core::graph::NodeId, out: &mut Vec<String>) {
    if let Some(n) = g.node(id) {
        if let Some(name) = node_name(n) {
            out.push(name);
        }
    }
}

fn node_name(n: &Node) -> Option<String> {
    match &n.kind {
        NodeKind::Function { name, .. } => Some(name.clone()),
        NodeKind::Type { name } => Some(name.clone()),
        NodeKind::State { name, .. } => Some(name.clone()),
        NodeKind::ApiEndpoint { method, path } => Some(format!("{method} {path}")),
        NodeKind::File { .. } => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use floe_core::artifact::PrRef;
    use floe_core::graph::{Edge, EdgeId, EdgeKind, NodeId, Span};
    use floe_core::provenance::Provenance;

    fn prov() -> Provenance {
        Provenance::new("test", "0", "t", b"")
    }

    fn fn_node(id: u32, qname: &str) -> Node {
        Node {
            id: NodeId(id),
            kind: NodeKind::Function {
                name: qname.into(),
                signature: format!("{qname}()"),
            },
            file: "src/t.ts".into(),
            span: Span { start: 0, end: 1 },
            provenance: prov(),
            package: None,
        }
    }

    fn api_hunk(hid: &str, node_id: u32) -> Hunk {
        Hunk {
            id: hid.into(),
            kind: HunkKind::Api {
                node: NodeId(node_id),
                before_signature: Some("old".into()),
                after_signature: Some("new".into()),
            },
            provenance: prov(),
        }
    }

    fn call_hunk(hid: &str, added_edge_ids: &[u32]) -> Hunk {
        Hunk {
            id: hid.into(),
            kind: HunkKind::Call {
                added_edges: added_edge_ids.iter().copied().map(EdgeId).collect(),
                removed_edges: Vec::new(),
            },
            provenance: prov(),
        }
    }

    fn seeded_artifact() -> Artifact {
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        // Two class-methods under `Queue.*` and one top-level helper.
        a.head.nodes = vec![
            fn_node(1, "Queue.enqueue"),
            fn_node(2, "Queue.dequeue"),
            fn_node(3, "formatTimestamp"),
        ];
        a.base.nodes = a.head.nodes.clone();
        a.hunks = vec![
            api_hunk("hunk-1", 1),
            api_hunk("hunk-2", 2),
            api_hunk("hunk-3", 3),
        ];
        a
    }

    /// Invariant (RFC §4a #1): every hunk appears in ≥ 1 flow. The
    /// structural clustering is the floor — if this invariant breaks,
    /// fallback behaviour downstream is silently wrong.
    #[test]
    fn every_hunk_appears_in_some_flow() {
        let a = seeded_artifact();
        let flows = cluster(&a);
        let mut seen = std::collections::HashSet::new();
        for f in &flows {
            for h in &f.hunk_ids {
                seen.insert(h.clone());
            }
        }
        for h in &a.hunks {
            assert!(
                seen.contains(&h.id),
                "hunk {} fell out of structural clustering",
                h.id
            );
        }
    }

    #[test]
    fn shared_class_prefix_groups_methods() {
        let a = seeded_artifact();
        let flows = cluster(&a);
        let queue_flow = flows
            .iter()
            .find(|f| f.name.contains("Queue"))
            .expect("expected a Queue.* flow");
        assert_eq!(queue_flow.hunk_ids.len(), 2);
        assert!(queue_flow.entities.iter().any(|e| e == "Queue.enqueue"));
        assert!(queue_flow.entities.iter().any(|e| e == "Queue.dequeue"));
    }

    #[test]
    fn all_flows_are_structurally_sourced() {
        let a = seeded_artifact();
        let flows = cluster(&a);
        for f in &flows {
            assert!(
                matches!(f.source, FlowSource::Structural),
                "structural pass must not emit FlowSource::Llm"
            );
        }
    }

    #[test]
    fn flow_id_is_deterministic_across_runs() {
        let a = seeded_artifact();
        let f1 = cluster(&a);
        let f2 = cluster(&a);
        let ids1: Vec<&str> = f1.iter().map(|f| f.id.as_str()).collect();
        let ids2: Vec<&str> = f2.iter().map(|f| f.id.as_str()).collect();
        assert_eq!(
            ids1, ids2,
            "flow IDs must be deterministic so the cache + reviewer links stay stable"
        );
    }

    #[test]
    fn top_level_bucket_catches_orphan_helper() {
        let a = seeded_artifact();
        let flows = cluster(&a);
        let top = flows
            .iter()
            .find(|f| f.name.contains("top-level") || f.name.contains("formatTimestamp"))
            .expect("top-level helper bucket missing");
        assert!(top.hunk_ids.iter().any(|h| h == "hunk-3"));
    }

    /// Propagation edges surface 1-hop callers/callees that reach a
    /// flow's entities without being in the flow themselves.
    #[test]
    fn propagation_edges_span_flow_boundary() {
        let mut a = seeded_artifact();
        // formatTimestamp calls Queue.enqueue — reviewer should see
        // that propagation on the Queue flow.
        a.head.edges.push(Edge {
            id: EdgeId(1),
            from: NodeId(3),
            to: NodeId(1),
            kind: EdgeKind::Calls,
            provenance: prov(),
        });
        let flows = cluster(&a);
        let queue_flow = flows.iter().find(|f| f.name.contains("Queue")).unwrap();
        assert!(
            queue_flow
                .propagation_edges
                .iter()
                .any(|(from, to)| from == "formatTimestamp" && to == "Queue.enqueue"),
            "expected formatTimestamp → Queue.enqueue propagation edge"
        );
    }

    #[test]
    fn empty_artifact_produces_no_flows() {
        let a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        assert!(cluster(&a).is_empty());
    }

    #[test]
    fn call_hunk_without_resolvable_edges_still_assigned_to_top_level() {
        let mut a = seeded_artifact();
        // A call hunk whose edge IDs don't resolve to any edge in
        // head/base (common when parse is partial). The entity list
        // ends up empty — classify_bucket should fall through to
        // "top-level" rather than skipping the hunk entirely.
        a.hunks.push(call_hunk("hunk-orphan", &[999]));
        let flows = cluster(&a);
        let seen: Vec<&str> = flows
            .iter()
            .flat_map(|f| f.hunk_ids.iter().map(String::as_str))
            .collect();
        assert!(
            seen.contains(&"hunk-orphan"),
            "unresolved-edge hunk must still appear in some flow"
        );
    }
}
