use floe_core::{Artifact, Flow, FlowSource};
use serde::{Deserialize, Serialize};

use crate::errors::{ErrorCode, ToolError};

/// The default per-run tool-call budget. Frozen at scope 3 week 6.
pub const DEFAULT_CALL_BUDGET: u32 = 200;

/// Flow names reserved for the `misc` fallback bucket the LLM must not
/// produce. Kept lower-cased; checked after ASCII-lowercasing the input.
pub const RESERVED_NAMES: &[&str] =
    &["misc", "various", "other", "unknown", "cluster", "group"];

pub const NAME_MIN: usize = 3;
pub const NAME_MAX: usize = 48;
pub const RATIONALE_MIN: usize = 1;
pub const RATIONALE_MAX: usize = 240;

/// A single LLM synthesis run against a seed artifact.
///
/// Holds the read-only artifact, a working flow list that starts as the
/// structural clusters (so the model can mutate / remove / rename them in
/// place), and a tool-call counter enforced against the per-run budget.
pub struct Session {
    pub(crate) artifact: Artifact,
    /// What `list_flows_initial()` returns — a frozen snapshot of the
    /// structural clustering. Mutations never touch this list.
    pub(crate) initial: Vec<Flow>,
    /// The working list. Starts as a clone of `initial`; every mutation
    /// tool operates on this. `finalize` promotes it.
    pub(crate) working: Vec<Flow>,
    pub(crate) call_count: u32,
    pub(crate) max_calls: u32,
    pub(crate) next_flow_index: u32,
}

impl Session {
    /// Build a new session. The artifact's current `flows[]` is taken as
    /// the structural starting point; it must not be empty. Use
    /// [`Session::new_relaxed`] for probe / navigation sessions that
    /// operate on a side-only artifact with no flows.
    pub fn new(artifact: Artifact) -> Result<Self, SessionInitError> {
        if artifact.flows.is_empty() {
            return Err(SessionInitError::NoStructuralFlows);
        }
        Ok(Self::new_relaxed(artifact))
    }

    /// Like [`Session::new`] but accepts an artifact with no flows —
    /// intended for read-only navigation probes that only need
    /// `list_entities` / `get_entity` / `neighbors` against one
    /// snapshot. Mutation tools still work but the flow invariants
    /// (coverage, reserved names) won't catch much against an empty
    /// working set.
    pub fn new_relaxed(artifact: Artifact) -> Self {
        let initial = artifact.flows.clone();
        let working = initial.clone();
        Self {
            artifact,
            initial,
            working,
            call_count: 0,
            max_calls: DEFAULT_CALL_BUDGET,
            next_flow_index: 0,
        }
    }

    /// Override the call budget. Useful in tests; in production the
    /// contract-frozen [`DEFAULT_CALL_BUDGET`] applies.
    pub fn with_call_budget(mut self, max: u32) -> Self {
        self.max_calls = max;
        self
    }

    /// Increment the call counter; return `CallBudgetExceeded` if over.
    pub(crate) fn charge_call(&mut self) -> Result<(), ToolError> {
        if self.call_count >= self.max_calls {
            return Err(ToolError::new(
                ErrorCode::CallBudgetExceeded,
                format!("call budget of {} exceeded", self.max_calls),
            ));
        }
        self.call_count += 1;
        Ok(())
    }

    /// Mint a fresh flow id for a freshly proposed flow. Counter-based so
    /// tests can assert deterministic ids.
    pub(crate) fn mint_flow_id(&mut self) -> String {
        let id = format!("flow-llm-{}", self.next_flow_index);
        self.next_flow_index += 1;
        id
    }

    /// Build a fresh Flow with the given name/rationale/hunks/entities
    /// and `source = Llm`. `model` and `version` are filled at finalize.
    pub(crate) fn make_flow(
        &mut self,
        name: String,
        rationale: String,
        hunk_ids: Vec<String>,
        extra_entities: Vec<String>,
    ) -> Flow {
        let id = self.mint_flow_id();
        // Entities = the unique qualified names of every hunk's touched
        // entities, plus the extra ones the LLM called out. Dedupe in place.
        let mut entities: Vec<String> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for hid in &hunk_ids {
            for e in entities_of_hunk(&self.artifact, hid) {
                if seen.insert(e.clone()) {
                    entities.push(e);
                }
            }
        }
        Flow {
            id,
            name,
            rationale,
            source: FlowSource::Llm {
                model: String::new(),
                version: String::new(),
            },
            hunk_ids,
            entities,
            extra_entities,
            propagation_edges: Vec::new(),
            order: 0,
            evidence: Vec::new(),
            cost: None,
            intent_fit: None,
            proof: None,
        }
    }

    pub fn artifact(&self) -> &Artifact {
        &self.artifact
    }
    pub fn initial_flows(&self) -> &[Flow] {
        &self.initial
    }
    pub fn working_flows(&self) -> &[Flow] {
        &self.working
    }
    pub fn call_count(&self) -> u32 {
        self.call_count
    }

    /* --- ergonomic method facades over the handler functions ------------- */

    pub fn list_hunks(&mut self) -> Result<Vec<crate::wire::HunkSummary>, ToolError> {
        crate::handlers::list_hunks(self)
    }
    pub fn list_entities(
        &mut self,
        side: Option<crate::wire::SnapshotSide>,
        kind: Option<crate::wire::EntityKindTag>,
    ) -> Result<Vec<crate::wire::EntityDescriptor>, ToolError> {
        crate::handlers::list_entities(self, side, kind)
    }
    pub fn get_entity(
        &mut self,
        id: &str,
    ) -> Result<crate::wire::EntityDescriptor, ToolError> {
        crate::handlers::get_entity(self, id)
    }
    pub fn neighbors(
        &mut self,
        id: &str,
        hops: u32,
    ) -> Result<crate::wire::NeighborsResponse, ToolError> {
        crate::handlers::neighbors(self, id, hops)
    }
    pub fn list_flows_initial(
        &mut self,
    ) -> Result<Vec<crate::wire::FlowInitial>, ToolError> {
        crate::handlers::list_flows_initial(self)
    }
    pub fn propose_flow(
        &mut self,
        name: &str,
        rationale: &str,
        hunk_ids: Vec<String>,
        extra_entities: Vec<String>,
    ) -> Result<String, ToolError> {
        crate::handlers::propose_flow(self, name, rationale, hunk_ids, extra_entities)
    }
    pub fn mutate_flow(
        &mut self,
        flow_id: &str,
        patch: crate::wire::MutateFlowPatch,
    ) -> Result<(), ToolError> {
        crate::handlers::mutate_flow(self, flow_id, patch)
    }
    pub fn remove_flow(&mut self, flow_id: &str) -> Result<(), ToolError> {
        crate::handlers::remove_flow(self, flow_id)
    }
    pub fn finalize(
        &mut self,
        model: &str,
        version: &str,
    ) -> crate::wire::FinalizeOutcome {
        crate::invariants::finalize(self, model, version)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
pub enum SessionInitError {
    #[error("artifact must already carry structural flows before LLM synthesis runs")]
    NoStructuralFlows,
}

/// Collect the qualified-name entities touched by a hunk (both sides).
/// Returns empty if the hunk id is unknown — callers are expected to
/// validate ids separately via [`super::handlers::has_hunk`].
pub(crate) fn entities_of_hunk(artifact: &Artifact, hunk_id: &str) -> Vec<String> {
    use floe_core::hunks::HunkKind;
    let Some(h) = artifact.hunks.iter().find(|h| h.id == hunk_id) else {
        return Vec::new();
    };
    let mut out: Vec<String> = Vec::new();
    match &h.kind {
        HunkKind::Call {
            added_edges,
            removed_edges,
        } => {
            for eid in added_edges.iter() {
                if let Some(e) = artifact.head.edges.iter().find(|e| &e.id == eid) {
                    if let Some(n) = qualified_name(&artifact.head, e.from) {
                        out.push(n);
                    }
                    if let Some(n) = qualified_name(&artifact.head, e.to) {
                        out.push(n);
                    }
                }
            }
            for eid in removed_edges.iter() {
                if let Some(e) = artifact.base.edges.iter().find(|e| &e.id == eid) {
                    if let Some(n) = qualified_name(&artifact.base, e.from) {
                        out.push(n);
                    }
                    if let Some(n) = qualified_name(&artifact.base, e.to) {
                        out.push(n);
                    }
                }
            }
        }
        HunkKind::State { node, .. } | HunkKind::Api { node, .. } => {
            if let Some(n) = qualified_name(&artifact.head, *node)
                .or_else(|| qualified_name(&artifact.base, *node))
            {
                out.push(n);
            }
        }
        HunkKind::Lock { file, primitive, .. } => {
            out.push(format!("{}:{}", file, primitive));
        }
        HunkKind::Data { file, type_name, .. } => {
            out.push(format!("{}:{}", file, type_name));
        }
        HunkKind::Docs { file, target, .. } => {
            out.push(format!("{}:{}", file, target));
        }
        HunkKind::Deletion { entity_name, .. } => {
            out.push(entity_name.clone());
        }
    }
    out
}

pub(crate) fn qualified_name(
    graph: &floe_core::Graph,
    id: floe_core::NodeId,
) -> Option<String> {
    let n = graph.nodes.iter().find(|n| n.id == id)?;
    use floe_core::NodeKind;
    Some(match &n.kind {
        NodeKind::Function { name, .. } => name.clone(),
        NodeKind::Type { name } => name.clone(),
        NodeKind::State { name, .. } => name.clone(),
        NodeKind::ApiEndpoint { method, path } => format!("{method} {path}"),
        NodeKind::File { path } => path.clone(),
    })
}
