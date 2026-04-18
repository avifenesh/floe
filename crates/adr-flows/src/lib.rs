//! Structural flow clustering — the deterministic floor.
//!
//! A flow here is the smallest useful group of hunks that "probably belong
//! together". We group by shared qualified-name prefix (class or top-level
//! bucket). This is intentionally sloppy — it catches the common case
//! (all `Queue.*` method changes end up together) and lets the LLM refine
//! via the `adr` PI extension. When LLM synthesis is off or rejected, the
//! UI renders these clusters with a visible "structural only" banner.

use std::collections::BTreeMap;

use adr_core::flow::{Flow, FlowSource};
use adr_core::graph::{Graph, Node, NodeKind};
use adr_core::hunks::{Hunk, HunkKind};
use adr_core::Artifact;

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
    by_prefix
        .into_iter()
        .enumerate()
        .map(|(i, (bucket, refs))| make_flow(i, &bucket, &refs))
        .collect()
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
        order: index as u32,
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
    }
    out.sort();
    out.dedup();
    out
}

fn collect_name(g: &Graph, id: adr_core::graph::NodeId, out: &mut Vec<String>) {
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
