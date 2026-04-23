//! The eight tool handlers the LLM calls through the wire transport.
//!
//! Each mutation handler charges the call budget **first**, then validates,
//! then (on success) applies to [`Session::working`]. Read handlers also
//! charge — the budget covers every tool call to the host, not just writes.

use floe_core::{Flow, FlowSource, Graph, NodeKind};

use crate::errors::{ErrorCode, ToolError};
use crate::state::{
    entities_of_hunk, qualified_name, Session, NAME_MAX, NAME_MIN, RATIONALE_MAX, RATIONALE_MIN,
    RESERVED_NAMES,
};
use crate::wire::{
    EntityDescriptor, EntityKindTag, FlowInitial, HunkKindTag, HunkSummary, MutateFlowPatch,
    NeighborEdge, NeighborEdgeKind, NeighborsResponse, Side, SnapshotSide, SpanDto,
};

/* -------------------------------------------------------------------------- */
/* Read tools                                                                 */
/* -------------------------------------------------------------------------- */

pub fn list_hunks(s: &mut Session) -> Result<Vec<HunkSummary>, ToolError> {
    s.charge_call()?;
    Ok(s.artifact
        .hunks
        .iter()
        .map(|h| hunk_to_summary(s, h))
        .collect())
}

/// Enumerate every entity in the artifact on the given side (or both).
/// Navigation tool — used by the probe pass to bootstrap "map the public
/// API of this repo" style questions where the model needs a starting
/// set of qualified names. Kept out of the synthesis prompt (which works
/// from hunks) but exposed to every caller that talks MCP.
pub fn list_entities(
    s: &mut Session,
    side: Option<SnapshotSide>,
    kind: Option<EntityKindTag>,
) -> Result<Vec<EntityDescriptor>, ToolError> {
    s.charge_call()?;
    let mut out: Vec<EntityDescriptor> = Vec::new();
    if side.is_none() || side == Some(SnapshotSide::Head) {
        for n in &s.artifact.head.nodes {
            let d = node_to_descriptor(n, SnapshotSide::Head);
            if kind.map_or(true, |k| k == d.kind) {
                out.push(d);
            }
        }
    }
    if side.is_none() || side == Some(SnapshotSide::Base) {
        for n in &s.artifact.base.nodes {
            let d = node_to_descriptor(n, SnapshotSide::Base);
            if kind.map_or(true, |k| k == d.kind) {
                out.push(d);
            }
        }
    }
    Ok(out)
}

pub fn get_entity(s: &mut Session, id: &str) -> Result<EntityDescriptor, ToolError> {
    s.charge_call()?;
    if let Some(d) = entity_by_qualified_name(&s.artifact.head, id, SnapshotSide::Head) {
        return Ok(d);
    }
    if let Some(d) = entity_by_qualified_name(&s.artifact.base, id, SnapshotSide::Base) {
        return Ok(d);
    }
    Err(ToolError::new(
        ErrorCode::EntityNotFound,
        format!("no entity with qualified name `{id}`"),
    ))
}

pub fn neighbors(
    s: &mut Session,
    id: &str,
    hops: u32,
) -> Result<NeighborsResponse, ToolError> {
    s.charge_call()?;
    let hops = hops.min(3);
    let (graph, side) = match find_entity(&s.artifact.head, id) {
        Some(_) => (&s.artifact.head, SnapshotSide::Head),
        None => match find_entity(&s.artifact.base, id) {
            Some(_) => (&s.artifact.base, SnapshotSide::Base),
            None => {
                return Err(ToolError::new(
                    ErrorCode::EntityNotFound,
                    format!("no entity with qualified name `{id}`"),
                ));
            }
        },
    };

    // BFS by `calls` + `defines` + `exports`. Transitions stay self-loops.
    let start = find_entity(graph, id).unwrap().id;
    let mut visited: std::collections::HashSet<floe_core::NodeId> =
        std::collections::HashSet::new();
    let mut frontier = vec![start];
    visited.insert(start);
    for _ in 0..hops {
        let mut next = Vec::new();
        for n in &frontier {
            for e in graph.edges.iter().filter(|e| e.from == *n || e.to == *n) {
                let other = if e.from == *n { e.to } else { e.from };
                if visited.insert(other) {
                    next.push(other);
                }
            }
        }
        if next.is_empty() {
            break;
        }
        frontier = next;
    }

    let nodes: Vec<EntityDescriptor> = visited
        .iter()
        .filter_map(|nid| {
            let n = graph.nodes.iter().find(|x| x.id == *nid)?;
            Some(node_to_descriptor(n, side))
        })
        .collect();

    let edges: Vec<NeighborEdge> = graph
        .edges
        .iter()
        .filter(|e| visited.contains(&e.from) && visited.contains(&e.to))
        .filter_map(|e| {
            let from = qualified_name(graph, e.from)?;
            let to = qualified_name(graph, e.to)?;
            let kind = match &e.kind {
                floe_core::EdgeKind::Calls => NeighborEdgeKind::Calls,
                floe_core::EdgeKind::Defines => NeighborEdgeKind::Defines,
                floe_core::EdgeKind::Exports => NeighborEdgeKind::Exports,
                floe_core::EdgeKind::Transitions { from: f, to: t } => {
                    NeighborEdgeKind::Transitions {
                        from: f.clone(),
                        to: t.clone(),
                    }
                }
            };
            Some(NeighborEdge { from, to, kind })
        })
        .collect();

    Ok(NeighborsResponse { nodes, edges })
}

pub fn list_flows_initial(s: &mut Session) -> Result<Vec<FlowInitial>, ToolError> {
    s.charge_call()?;
    Ok(s.initial
        .iter()
        .map(|f| FlowInitial {
            id: f.id.clone(),
            name: f.name.clone(),
            rationale: f.rationale.clone(),
            hunk_ids: f.hunk_ids.clone(),
            entities: f.entities.clone(),
            confidence: "structural".into(),
        })
        .collect())
}

/* -------------------------------------------------------------------------- */
/* Mutation tools                                                             */
/* -------------------------------------------------------------------------- */

pub fn propose_flow(
    s: &mut Session,
    name: &str,
    rationale: &str,
    hunk_ids: Vec<String>,
    extra_entities: Vec<String>,
) -> Result<String, ToolError> {
    s.charge_call()?;
    validate_name(name)?;
    validate_rationale(rationale)?;
    for hid in &hunk_ids {
        if !has_hunk(s, hid) {
            return Err(ToolError::new(
                ErrorCode::HunkNotFound,
                format!("hunk `{hid}` not in artifact"),
            ));
        }
    }
    for e in &extra_entities {
        if !has_entity(s, e) {
            return Err(ToolError::new(
                ErrorCode::EntityNotFound,
                format!("entity `{e}` not in artifact"),
            ));
        }
    }
    let flow = s.make_flow(
        name.to_string(),
        rationale.to_string(),
        hunk_ids,
        extra_entities,
    );
    let id = flow.id.clone();
    s.working.push(flow);
    Ok(id)
}

pub fn mutate_flow(
    s: &mut Session,
    flow_id: &str,
    patch: MutateFlowPatch,
) -> Result<(), ToolError> {
    s.charge_call()?;

    // Pre-validate referenced ids (before we mutate, so we can reject atomically).
    if let Some(ref n) = patch.name {
        validate_name(n)?;
    }
    if let Some(ref r) = patch.rationale {
        validate_rationale(r)?;
    }
    for hid in patch.add_hunks.iter().chain(patch.remove_hunks.iter()) {
        if !has_hunk(s, hid) {
            return Err(ToolError::new(
                ErrorCode::HunkNotFound,
                format!("hunk `{hid}` not in artifact"),
            ));
        }
    }
    for e in patch.add_entities.iter().chain(patch.remove_entities.iter()) {
        if !has_entity(s, e) {
            return Err(ToolError::new(
                ErrorCode::EntityNotFound,
                format!("entity `{e}` not in artifact"),
            ));
        }
    }

    let idx = s
        .working
        .iter()
        .position(|f| f.id == flow_id)
        .ok_or_else(|| {
            ToolError::new(
                ErrorCode::FlowNotFound,
                format!("no flow with id `{flow_id}` in working set"),
            )
        })?;

    let f = &mut s.working[idx];
    if let Some(n) = patch.name {
        f.name = n;
    }
    if let Some(r) = patch.rationale {
        f.rationale = r;
    }
    for hid in patch.add_hunks {
        if !f.hunk_ids.contains(&hid) {
            f.hunk_ids.push(hid);
        }
    }
    if !patch.remove_hunks.is_empty() {
        let rm: std::collections::HashSet<&String> = patch.remove_hunks.iter().collect();
        f.hunk_ids.retain(|h| !rm.contains(h));
    }
    for e in patch.add_entities {
        if !f.extra_entities.contains(&e) && !f.entities.contains(&e) {
            f.extra_entities.push(e);
        }
    }
    if !patch.remove_entities.is_empty() {
        let rm: std::collections::HashSet<&String> = patch.remove_entities.iter().collect();
        f.extra_entities.retain(|e| !rm.contains(e));
    }
    // Keep the derived `entities` list in sync with `hunk_ids` — the LLM
    // doesn't manage it directly. `extra_entities` is the LLM's knob.
    let mut derived: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    for hid in &f.hunk_ids {
        for e in entities_of_hunk(&s.artifact, hid) {
            if seen.insert(e.clone()) {
                derived.push(e);
            }
        }
    }
    f.entities = derived;
    Ok(())
}

pub fn remove_flow(s: &mut Session, flow_id: &str) -> Result<(), ToolError> {
    s.charge_call()?;
    let idx = s
        .working
        .iter()
        .position(|f| f.id == flow_id)
        .ok_or_else(|| {
            ToolError::new(
                ErrorCode::FlowNotFound,
                format!("no flow with id `{flow_id}` in working set"),
            )
        })?;

    // Coverage pre-check: every hunk in this flow must still be covered
    // by another flow after removal. Otherwise reject.
    let victim = &s.working[idx];
    let my_hunks: std::collections::HashSet<&String> = victim.hunk_ids.iter().collect();
    let covered_elsewhere: std::collections::HashSet<&String> = s
        .working
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != idx)
        .flat_map(|(_, f)| f.hunk_ids.iter())
        .collect();
    let orphaned: Vec<&&String> = my_hunks.difference(&covered_elsewhere).collect();
    if !orphaned.is_empty() {
        let sample = orphaned.iter().take(3).map(|s| s.as_str()).collect::<Vec<_>>();
        return Err(ToolError::new(
            ErrorCode::CoverageBroken,
            format!(
                "removing `{flow_id}` would orphan {} hunk(s): {}",
                orphaned.len(),
                sample.join(", ")
            ),
        ));
    }

    s.working.remove(idx);
    Ok(())
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

pub(crate) fn has_hunk(s: &Session, hid: &str) -> bool {
    s.artifact.hunks.iter().any(|h| h.id == hid)
}

pub(crate) fn has_entity(s: &Session, qname: &str) -> bool {
    find_entity(&s.artifact.head, qname).is_some() || find_entity(&s.artifact.base, qname).is_some()
}

fn find_entity<'a>(graph: &'a Graph, qname: &str) -> Option<&'a floe_core::Node> {
    graph.nodes.iter().find(|n| match &n.kind {
        NodeKind::Function { name, .. } => name == qname,
        NodeKind::Type { name } => name == qname,
        NodeKind::State { name, .. } => name == qname,
        NodeKind::ApiEndpoint { method, path } => format!("{method} {path}") == qname,
        NodeKind::File { path } => path == qname,
    })
}

fn entity_by_qualified_name(
    graph: &Graph,
    qname: &str,
    side: SnapshotSide,
) -> Option<EntityDescriptor> {
    let n = find_entity(graph, qname)?;
    Some(node_to_descriptor(n, side))
}

fn node_to_descriptor(n: &floe_core::Node, side: SnapshotSide) -> EntityDescriptor {
    let (kind, name, signature) = match &n.kind {
        NodeKind::Function { name, signature } => {
            (EntityKindTag::Function, name.clone(), Some(signature.clone()))
        }
        NodeKind::Type { name } => (EntityKindTag::Type, name.clone(), None),
        NodeKind::State { name, .. } => (EntityKindTag::State, name.clone(), None),
        NodeKind::ApiEndpoint { method, path } => (
            EntityKindTag::ApiEndpoint,
            format!("{method} {path}"),
            None,
        ),
        NodeKind::File { path } => (EntityKindTag::File, path.clone(), None),
    };
    EntityDescriptor {
        id: name.clone(),
        kind,
        name,
        file: n.file.clone(),
        span: SpanDto {
            start: n.span.start,
            end: n.span.end,
        },
        side,
        signature,
    }
}

fn hunk_to_summary(s: &Session, h: &floe_core::Hunk) -> HunkSummary {
    use floe_core::hunks::HunkKind;
    let entities = entities_of_hunk(&s.artifact, &h.id);
    let (kind, summary, side) = match &h.kind {
        HunkKind::Call {
            added_edges,
            removed_edges,
        } => {
            let side = match (added_edges.is_empty(), removed_edges.is_empty()) {
                (false, true) => Side::Added,
                (true, false) => Side::Removed,
                _ => Side::Both,
            };
            let summary = format!(
                "call edges: +{} / -{}",
                added_edges.len(),
                removed_edges.len()
            );
            (HunkKindTag::Call, summary, side)
        }
        HunkKind::State {
            node,
            added_variants,
            removed_variants,
        } => {
            let side = match (added_variants.is_empty(), removed_variants.is_empty()) {
                (false, true) => Side::Added,
                (true, false) => Side::Removed,
                _ => Side::Both,
            };
            let name = qualified_name(&s.artifact.head, *node)
                .or_else(|| qualified_name(&s.artifact.base, *node))
                .unwrap_or_default();
            let summary = format!(
                "state `{name}`: +{} / -{} variant(s)",
                added_variants.len(),
                removed_variants.len()
            );
            (HunkKindTag::State, summary, side)
        }
        HunkKind::Api {
            node,
            before_signature,
            after_signature,
        } => {
            let side = match (after_signature.is_some(), before_signature.is_some()) {
                (true, false) => Side::Added,
                (false, true) => Side::Removed,
                _ => Side::Both,
            };
            let name = qualified_name(&s.artifact.head, *node)
                .or_else(|| qualified_name(&s.artifact.base, *node))
                .unwrap_or_default();
            let summary = format!("api `{name}` signature changed");
            (HunkKindTag::Api, summary, side)
        }
        HunkKind::Lock {
            file,
            primitive,
            before,
            after,
        } => {
            let side = match (after.is_some(), before.is_some()) {
                (true, false) => Side::Added,
                (false, true) => Side::Removed,
                _ => Side::Both,
            };
            let summary = format!("lock `{primitive}` in {file}");
            (HunkKindTag::Lock, summary, side)
        }
        HunkKind::Data {
            file,
            type_name,
            added_fields,
            removed_fields,
            renamed_fields,
        } => {
            let side = match (!added_fields.is_empty(), !removed_fields.is_empty()) {
                (true, false) => Side::Added,
                (false, true) => Side::Removed,
                _ => Side::Both,
            };
            let summary = format!(
                "data `{type_name}` in {file}: +{}/-{} fields, {} renamed",
                added_fields.len(),
                removed_fields.len(),
                renamed_fields.len()
            );
            (HunkKindTag::Data, summary, side)
        }
        HunkKind::Docs { file, target, drift_kind } => {
            let summary = format!("docs drift ({drift_kind}) on `{target}` in {file}");
            (HunkKindTag::Docs, summary, Side::Both)
        }
        HunkKind::Deletion { file, entity_name, was_exported } => {
            let marker = if *was_exported { "exported " } else { "" };
            let summary = format!("deletion: {marker}`{entity_name}` in {file}");
            (HunkKindTag::Deletion, summary, Side::Removed)
        }
    };
    HunkSummary {
        id: h.id.clone(),
        kind,
        summary,
        entities,
        side,
    }
}

fn validate_name(n: &str) -> Result<(), ToolError> {
    if n.len() < NAME_MIN {
        return Err(ToolError::new(
            ErrorCode::NameTooShort,
            format!("name must be at least {NAME_MIN} chars"),
        ));
    }
    if n.len() > NAME_MAX {
        return Err(ToolError::new(
            ErrorCode::NameTooLong,
            format!("name must be at most {NAME_MAX} chars"),
        ));
    }
    let lower = n.to_ascii_lowercase();
    if RESERVED_NAMES.iter().any(|r| *r == lower) {
        return Err(ToolError::new(
            ErrorCode::NameReserved,
            format!("`{n}` is reserved for the misc fallback bucket"),
        ));
    }
    Ok(())
}

fn validate_rationale(r: &str) -> Result<(), ToolError> {
    if r.len() < RATIONALE_MIN {
        return Err(ToolError::new(
            ErrorCode::RationaleTooShort,
            "rationale must not be empty",
        ));
    }
    if r.len() > RATIONALE_MAX {
        return Err(ToolError::new(
            ErrorCode::RationaleTooLong,
            format!("rationale must be at most {RATIONALE_MAX} chars"),
        ));
    }
    Ok(())
}

/* -------------------------------------------------------------------------- */
/* Finalize                                                                   */
/* -------------------------------------------------------------------------- */

/// Tag the working flows as accepted. Called only from `finalize` after
/// invariants pass; re-stamps every flow with the model + runtime version
/// and assigns stable `order` values.
pub(crate) fn stamp_accepted(s: &mut Session, model: &str, version: &str) -> Vec<Flow> {
    let mut out: Vec<Flow> = s.working.clone();
    for (i, f) in out.iter_mut().enumerate() {
        f.order = i as u32;
        f.source = FlowSource::Llm {
            model: model.into(),
            version: version.into(),
        };
    }
    out
}
