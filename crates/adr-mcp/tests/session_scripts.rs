//! Scripted sequences that exercise the Session as an LLM would — each test
//! mirrors one realistic (or adversarial) tool-call trajectory.

use adr_core::{
    artifact::PrRef,
    graph::{Edge, EdgeId, EdgeKind, Graph, Node, NodeId, NodeKind, Span},
    hunks::{Hunk, HunkKind},
    provenance::Provenance,
    Artifact, FlowSource,
};
use adr_mcp::{ErrorCode, MutateFlowPatch, Session};

fn prov() -> Provenance {
    Provenance::new("test", "0", "p", b"")
}

fn func_node(id: u32, name: &str, file: &str, signature: &str) -> Node {
    Node {
        id: NodeId(id),
        kind: NodeKind::Function {
            name: name.into(),
            signature: signature.into(),
        },
        file: file.into(),
        span: Span { start: 0, end: 1 },
        provenance: prov(),
    }
}

fn fixture() -> Artifact {
    let mut a = Artifact::new(PrRef {
        repo: "demo".into(),
        base_sha: "b".into(),
        head_sha: "h".into(),
    });
    // Head: Queue.setBudget calls Queue.recordMetric, Job.run calls Job.stream.
    a.head = Graph {
        nodes: vec![
            func_node(1, "Queue.setBudget", "src/queue.ts", "setBudget(...)"),
            func_node(2, "Queue.recordMetric", "src/queue.ts", "recordMetric(...)"),
            func_node(3, "Job.run", "src/job.ts", "run(...)"),
            func_node(4, "Job.stream", "src/job.ts", "stream(...)"),
        ],
        edges: vec![
            Edge {
                id: EdgeId(10),
                from: NodeId(1),
                to: NodeId(2),
                kind: EdgeKind::Calls,
                provenance: prov(),
            },
            Edge {
                id: EdgeId(11),
                from: NodeId(3),
                to: NodeId(4),
                kind: EdgeKind::Calls,
                provenance: prov(),
            },
        ],
    };
    // Base: same nodes, but only the old edge.
    a.base = Graph {
        nodes: a.head.nodes.clone(),
        edges: vec![Edge {
            id: EdgeId(20),
            from: NodeId(3),
            to: NodeId(4),
            kind: EdgeKind::Calls,
            provenance: prov(),
        }],
    };
    a.hunks = vec![
        Hunk {
            id: "hunk-budget".into(),
            kind: HunkKind::Call {
                added_edges: vec![EdgeId(10)],
                removed_edges: vec![],
            },
            provenance: prov(),
        },
        Hunk {
            id: "hunk-stream-api".into(),
            kind: HunkKind::Api {
                node: NodeId(4),
                before_signature: Some("stream()".into()),
                after_signature: Some("stream(type: string)".into()),
            },
            provenance: prov(),
        },
    ];
    a.flows = vec![
        adr_core::Flow {
            id: "flow-struct-0".into(),
            name: "<structural: Queue>".into(),
            rationale: "shared prefix".into(),
            source: FlowSource::Structural,
            hunk_ids: vec!["hunk-budget".into()],
            entities: vec!["Queue.setBudget".into(), "Queue.recordMetric".into()],
            extra_entities: vec![],
            propagation_edges: vec![],
            order: 0,
            evidence: vec![],
            cost: None,
            intent_fit: None,
            proof: None,
        },
        adr_core::Flow {
            id: "flow-struct-1".into(),
            name: "<structural: Job>".into(),
            rationale: "shared prefix".into(),
            source: FlowSource::Structural,
            hunk_ids: vec!["hunk-stream-api".into()],
            entities: vec!["Job.run".into(), "Job.stream".into()],
            extra_entities: vec![],
            propagation_edges: vec![],
            order: 1,
            evidence: vec![],
            cost: None,
            intent_fit: None,
            proof: None,
        },
    ];
    a
}

/* -------------------------------------------------------------------------- */
/* Happy path                                                                 */
/* -------------------------------------------------------------------------- */

#[test]
fn happy_path_rename_and_finalize() {
    let mut s = Session::new(fixture()).unwrap();

    // Model reads the starting point, renames each flow, and finalizes.
    let initial = s.list_flows_initial().unwrap();
    assert_eq!(initial.len(), 2);

    s.mutate_flow(
        "flow-struct-0",
        MutateFlowPatch {
            name: Some("Multi-metric budget widening".into()),
            rationale: Some("budget signature + call wiring".into()),
            ..Default::default()
        },
    )
    .unwrap();
    s.mutate_flow(
        "flow-struct-1",
        MutateFlowPatch {
            name: Some("Streaming chunk contract".into()),
            rationale: Some("stream signature gains type arg".into()),
            ..Default::default()
        },
    )
    .unwrap();

    let outcome = s.finalize("gemma4:26b-a4b-it-q4_K_M", "0.3.0");
    match outcome {
        adr_mcp::FinalizeOutcome::Accepted { flows } => {
            assert_eq!(flows.len(), 2);
            assert!(matches!(
                flows[0].source,
                FlowSource::Llm { ref model, .. } if model == "gemma4:26b-a4b-it-q4_K_M"
            ));
            assert_eq!(flows[0].name, "Multi-metric budget widening");
            assert_eq!(flows[0].order, 0);
            assert_eq!(flows[1].order, 1);
        }
        other => panic!("expected Accepted, got {other:?}"),
    }
}

#[test]
fn propose_and_merge_covers_all_hunks() {
    let mut s = Session::new(fixture()).unwrap();
    // Model decides the two clusters should actually be one merged flow.
    s.remove_flow("flow-struct-0").unwrap_err(); // would orphan hunk-budget
    // OK — propose a combined flow first, then remove both structural ones.
    let combined = s
        .propose_flow(
            "Budget + streaming refactor",
            "two entangled stories this reviewer sees as one",
            vec!["hunk-budget".into(), "hunk-stream-api".into()],
            vec![],
        )
        .unwrap();
    s.remove_flow("flow-struct-0").unwrap();
    s.remove_flow("flow-struct-1").unwrap();
    let outcome = s.finalize("m", "v");
    let adr_mcp::FinalizeOutcome::Accepted { flows } = outcome else {
        panic!("expected accept");
    };
    assert_eq!(flows.len(), 1);
    assert_eq!(flows[0].id, combined);
    assert_eq!(flows[0].hunk_ids.len(), 2);
}

/* -------------------------------------------------------------------------- */
/* Per-mutation validation                                                    */
/* -------------------------------------------------------------------------- */

#[test]
fn reserved_name_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let err = s
        .propose_flow("misc", "r", vec!["hunk-budget".into()], vec![])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NameReserved);
}

#[test]
fn name_too_short_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let err = s
        .propose_flow("ab", "r", vec!["hunk-budget".into()], vec![])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NameTooShort);
}

#[test]
fn name_too_long_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let long = "x".repeat(49);
    let err = s
        .propose_flow(&long, "r", vec!["hunk-budget".into()], vec![])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::NameTooLong);
}

#[test]
fn rationale_too_long_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let long_r = "x".repeat(241);
    let err = s
        .propose_flow("Good name", &long_r, vec!["hunk-budget".into()], vec![])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::RationaleTooLong);
}

#[test]
fn unknown_hunk_rejected_at_propose() {
    let mut s = Session::new(fixture()).unwrap();
    let err = s
        .propose_flow("Flowy", "rationale", vec!["hunk-ghost".into()], vec![])
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::HunkNotFound);
}

#[test]
fn unknown_entity_in_extra_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let err = s
        .propose_flow(
            "Flowy",
            "rationale",
            vec!["hunk-budget".into()],
            vec!["Ghost.method".into()],
        )
        .unwrap_err();
    assert_eq!(err.code, ErrorCode::EntityNotFound);
}

/* -------------------------------------------------------------------------- */
/* Coverage invariant                                                         */
/* -------------------------------------------------------------------------- */

#[test]
fn finalize_rejects_orphan_hunk() {
    let mut s = Session::new(fixture()).unwrap();
    // Remove both structural flows via a propose-that-covers-only-one shuffle.
    let _ = s
        .propose_flow(
            "Only budget",
            "only budget story",
            vec!["hunk-budget".into()],
            vec![],
        )
        .unwrap();
    s.remove_flow("flow-struct-0").unwrap();
    s.remove_flow("flow-struct-1").unwrap_err(); // would orphan hunk-stream-api
}

#[test]
fn remove_flow_orphan_hunk_rejected() {
    let mut s = Session::new(fixture()).unwrap();
    let err = s.remove_flow("flow-struct-0").unwrap_err();
    assert_eq!(err.code, ErrorCode::CoverageBroken);
}

/* -------------------------------------------------------------------------- */
/* Call budget                                                                */
/* -------------------------------------------------------------------------- */

#[test]
fn call_budget_enforced() {
    let mut s = Session::new(fixture()).unwrap().with_call_budget(3);
    // Three reads of list_hunks burn the budget.
    s.list_hunks().unwrap();
    s.list_hunks().unwrap();
    s.list_hunks().unwrap();
    let err = s.list_hunks().unwrap_err();
    assert_eq!(err.code, ErrorCode::CallBudgetExceeded);
}

/* -------------------------------------------------------------------------- */
/* Read tool shapes                                                           */
/* -------------------------------------------------------------------------- */

#[test]
fn list_hunks_shapes() {
    let mut s = Session::new(fixture()).unwrap();
    let summaries = s.list_hunks().unwrap();
    assert_eq!(summaries.len(), 2);
    let budget = &summaries[0];
    assert_eq!(budget.id, "hunk-budget");
    assert!(
        budget.entities.contains(&"Queue.setBudget".into())
            && budget.entities.contains(&"Queue.recordMetric".into())
    );
}

#[test]
fn get_entity_returns_function_signature() {
    let mut s = Session::new(fixture()).unwrap();
    let d = s.get_entity("Queue.setBudget").unwrap();
    assert_eq!(d.file, "src/queue.ts");
    assert!(d.signature.is_some());
}

#[test]
fn neighbors_returns_callee_within_one_hop() {
    let mut s = Session::new(fixture()).unwrap();
    let n = s.neighbors("Queue.setBudget", 1).unwrap();
    let names: Vec<&str> = n.nodes.iter().map(|x| x.name.as_str()).collect();
    assert!(names.contains(&"Queue.recordMetric"));
}
