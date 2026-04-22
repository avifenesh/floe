//! Deterministic evidence pass.
//!
//! Runs after flows are finalised (either by the structural clusterer or
//! the LLM). Walks every flow and emits a short list of [`Claim`]s that
//! back or caution about the flow's rationale. Each collector is a pure
//! function: same artifact + same flow → same claims every time.
//!
//! Scope 5 starter set:
//!
//! 1. **SingleFile / CrossFile** — counts files the flow's hunks touch.
//! 2. **CallChain** — connectedness of Call hunks' edges.
//! 3. **SignatureConsistency** — do API hunks share a signature shape?
//! 4. **TestCoverage** — are there test files touching these entities?
//!
//! A human reviewer's first four questions about a flow. More collectors
//! slot in without schema changes.

use adr_core::{
    evidence::{Claim, ClaimKind, Strength},
    hunks::HunkKind,
    provenance::Provenance,
    Artifact, Flow,
};

const COLLECTOR_VERSION: &str = "0.1.0";

/// Run every collector over the artifact and return a new artifact with
/// populated `flow.evidence`. The input is consumed to avoid cloning the
/// (potentially large) base/head graphs.
pub fn collect(mut artifact: Artifact) -> Artifact {
    let mut flows = std::mem::take(&mut artifact.flows);
    for flow in flows.iter_mut() {
        flow.evidence = claims_for_flow(&artifact, flow);
    }
    artifact.flows = flows;
    artifact
}

/// Emit every claim that applies to a single flow.
fn claims_for_flow(artifact: &Artifact, flow: &Flow) -> Vec<Claim> {
    let mut out = Vec::new();
    if let Some(c) = file_scope_claim(artifact, flow) {
        out.push(c);
    }
    if let Some(c) = call_chain_claim(artifact, flow) {
        out.push(c);
    }
    if let Some(c) = signature_consistency_claim(artifact, flow) {
        out.push(c);
    }
    if let Some(c) = test_coverage_claim(artifact, flow) {
        out.push(c);
    }
    out
}

/* -------------------------------------------------------------------------- */
/* Collectors                                                                 */
/* -------------------------------------------------------------------------- */

/// One claim based on how many files the flow's hunks touch.
fn file_scope_claim(artifact: &Artifact, flow: &Flow) -> Option<Claim> {
    let files = files_of_flow(artifact, flow);
    match files.len() {
        0 => None,
        1 => {
            let only = files.into_iter().next().unwrap();
            Some(claim(
                flow,
                ClaimKind::SingleFile,
                format!("All hunks live in `{only}` — scope is local."),
                Strength::High,
                Vec::new(),
                "file-scope",
            ))
        }
        n @ 2..=3 => {
            let list = files.iter().cloned().collect::<Vec<_>>().join("`, `");
            Some(claim(
                flow,
                ClaimKind::CrossFile,
                format!("Touches {n} files: `{list}`."),
                Strength::Medium,
                Vec::new(),
                "file-scope",
            ))
        }
        n => {
            let preview = files.iter().take(3).cloned().collect::<Vec<_>>().join("`, `");
            Some(claim(
                flow,
                ClaimKind::CrossFile,
                format!("Touches {n} files ({preview}`, …) — review the fanout."),
                Strength::Low,
                Vec::new(),
                "file-scope",
            ))
        }
    }
}

/// Are the Call hunks connected? If every call edge shares an endpoint
/// with at least one other edge in the same flow, we emit a High-strength
/// "call chain" claim. Otherwise the edges are independent touches.
fn call_chain_claim(artifact: &Artifact, flow: &Flow) -> Option<Claim> {
    let mut endpoints: Vec<(String, String)> = Vec::new();
    for hid in &flow.hunk_ids {
        let Some(h) = artifact.hunks.iter().find(|x| x.id == *hid) else {
            continue;
        };
        let HunkKind::Call {
            added_edges,
            removed_edges,
        } = &h.kind
        else {
            continue;
        };
        for eid in added_edges {
            if let Some(e) = artifact.head.edges.iter().find(|e| &e.id == eid) {
                if let (Some(from), Some(to)) =
                    (qname(&artifact.head, e.from), qname(&artifact.head, e.to))
                {
                    endpoints.push((from, to));
                }
            }
        }
        for eid in removed_edges {
            if let Some(e) = artifact.base.edges.iter().find(|e| &e.id == eid) {
                if let (Some(from), Some(to)) =
                    (qname(&artifact.base, e.from), qname(&artifact.base, e.to))
                {
                    endpoints.push((from, to));
                }
            }
        }
    }
    if endpoints.is_empty() {
        return None;
    }
    if endpoints.len() == 1 {
        let (from, to) = endpoints.into_iter().next().unwrap();
        return Some(claim(
            flow,
            ClaimKind::CallChain,
            format!("One call edge: `{from}` → `{to}`."),
            Strength::Medium,
            Vec::new(),
            "call-chain",
        ));
    }

    // Two or more edges — check connectivity by shared endpoint. If each
    // edge shares at least one endpoint with another edge, the flow is a
    // chain; otherwise the edges are independent.
    let all_names: std::collections::HashSet<String> = endpoints
        .iter()
        .flat_map(|(a, b)| [a.clone(), b.clone()])
        .collect();
    let mut ep_count: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for n in &all_names {
        let c = endpoints
            .iter()
            .filter(|(a, b)| a == n || b == n)
            .count();
        ep_count.insert(n.as_str(), c);
    }
    let all_connected = endpoints
        .iter()
        .all(|(a, b)| ep_count.get(a.as_str()).copied().unwrap_or(0) >= 2
            || ep_count.get(b.as_str()).copied().unwrap_or(0) >= 2);
    if all_connected {
        Some(claim(
            flow,
            ClaimKind::CallChain,
            format!(
                "{} call edges form a connected chain — the flow is one runtime trajectory.",
                endpoints.len()
            ),
            Strength::High,
            Vec::new(),
            "call-chain",
        ))
    } else {
        Some(claim(
            flow,
            ClaimKind::CallChain,
            format!(
                "{} call edges but they don't all share endpoints — the flow may be several disjoint touches.",
                endpoints.len()
            ),
            Strength::Low,
            Vec::new(),
            "call-chain",
        ))
    }
}

/// Do the API hunks in this flow share a signature-change shape? We use
/// a crude proxy: do any pair of signatures share at least one "notable
/// token" (`Promise`, `async`, `undefined`, a parameter name that shows
/// up in multiple signatures)? Coarse but practical.
fn signature_consistency_claim(artifact: &Artifact, flow: &Flow) -> Option<Claim> {
    let mut sigs: Vec<(String, String)> = Vec::new();
    for hid in &flow.hunk_ids {
        let Some(h) = artifact.hunks.iter().find(|x| x.id == *hid) else {
            continue;
        };
        let HunkKind::Api {
            node,
            before_signature,
            after_signature,
        } = &h.kind
        else {
            continue;
        };
        let name = qname(&artifact.head, *node)
            .or_else(|| qname(&artifact.base, *node))
            .unwrap_or_default();
        if let (Some(b), Some(a)) = (before_signature, after_signature) {
            sigs.push((name.clone(), format!("{b} → {a}")));
        } else if let Some(a) = after_signature {
            sigs.push((name, format!("+ {a}")));
        } else if let Some(b) = before_signature {
            sigs.push((name, format!("- {b}")));
        }
    }
    if sigs.len() < 2 {
        return None;
    }
    // Pairwise token overlap — cheap signal.
    let tokenized: Vec<std::collections::HashSet<String>> = sigs
        .iter()
        .map(|(_, s)| {
            s.split(|c: char| !c.is_alphanumeric())
                .filter(|t| t.len() > 2)
                .map(|t| t.to_string())
                .collect()
        })
        .collect();
    let first = &tokenized[0];
    let shared: std::collections::HashSet<&String> = tokenized
        .iter()
        .skip(1)
        .fold(first.iter().collect(), |acc, s| {
            acc.intersection(&s.iter().collect())
                .copied()
                .collect()
        });
    if shared.len() >= 2 {
        let preview: Vec<String> = shared
            .iter()
            .take(3)
            .map(|s| format!("`{s}`"))
            .collect();
        Some(claim(
            flow,
            ClaimKind::SignatureConsistency,
            format!(
                "{} API signatures share tokens {} — consistent shape change.",
                sigs.len(),
                preview.join(", ")
            ),
            Strength::High,
            sigs.iter().map(|(n, _)| n.clone()).collect(),
            "signature-consistency",
        ))
    } else if sigs.len() >= 3 {
        Some(claim(
            flow,
            ClaimKind::SignatureConsistency,
            format!(
                "{} API signatures with little token overlap — double-check they belong together.",
                sigs.len()
            ),
            Strength::Low,
            sigs.iter().map(|(n, _)| n.clone()).collect(),
            "signature-consistency",
        ))
    } else {
        None
    }
}

/// Coverage signal from two sources: dedicated test files AND runnable
/// example files. Both count — an example that exercises the flow's
/// API in a realistic caller is often a stronger signal than a unit
/// test against a mock. (Avi's framing on glide-mq #181: "examples are
/// the best proof".)
///
/// Heuristic: collect every test + example file in the repo (whole
/// graph, not just the files the hunks touch), then name-match each
/// against tokens lifted from the flow's entity names. A test file
/// whose basename contains a token from a flow entity counts as
/// relevant coverage for that flow — e.g. `tests/budget.test.ts`
/// against a flow touching `Queue.getFlowBudget`.
fn test_coverage_claim(artifact: &Artifact, flow: &Flow) -> Option<Claim> {
    if flow.entities.is_empty() {
        return None;
    }

    let tokens = entity_name_tokens(&flow.entities);
    if tokens.is_empty() {
        return None;
    }

    let (mut total_tests, mut matching_tests) = (0usize, 0usize);
    let (mut total_examples, mut matching_examples) = (0usize, 0usize);
    let mut sample_tests: Vec<String> = Vec::new();
    let mut sample_examples: Vec<String> = Vec::new();
    let mut seen_files: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for graph in [&artifact.head, &artifact.base] {
        for n in &graph.nodes {
            if !seen_files.insert(n.file.clone()) {
                continue;
            }
            let is_test = is_test_path(&n.file);
            let is_example = is_example_path(&n.file);
            if !is_test && !is_example {
                continue;
            }
            if is_test {
                total_tests += 1;
            }
            if is_example {
                total_examples += 1;
            }
            let filename_lower = n
                .file
                .rsplit(['/', '\\'])
                .next()
                .unwrap_or(&n.file)
                .to_ascii_lowercase();
            let matched = tokens
                .iter()
                .any(|t| filename_lower.contains(t.as_str()));
            if !matched {
                continue;
            }
            if is_test {
                matching_tests += 1;
                if sample_tests.len() < 3 {
                    sample_tests.push(n.file.clone());
                }
            } else if is_example {
                matching_examples += 1;
                if sample_examples.len() < 3 {
                    sample_examples.push(n.file.clone());
                }
            }
        }
    }

    // Only surface a claim when we have real signal. Name-match →
    // High. Zero tests in the repo at all → Low (regression warning).
    // Ambiguous middle ("repo has tests but none name-match") is
    // skipped — it's worst-of-both-worlds: too weak to trust, but
    // loud enough to clutter the evidence list. The reviewer can
    // always check the test directory if they want to.
    let (strength, text) = if matching_tests > 0 {
        (
            Strength::High,
            format!(
                "{matching_tests} test file(s) name-match this flow's entities — e.g. `{}`.",
                sample_tests.first().cloned().unwrap_or_default()
            ),
        )
    } else if matching_examples > 0 {
        (
            Strength::High,
            format!(
                "{matching_examples} example file(s) exercise this flow's API — e.g. `{}`.",
                sample_examples.first().cloned().unwrap_or_default()
            ),
        )
    } else if total_tests == 0 && total_examples == 0 {
        (
            Strength::Low,
            "No test or example files found in the repo — regression risk is high.".into(),
        )
    } else {
        // Tests exist but don't name-match this flow; no reliable
        // signal to surface.
        return None;
    };

    Some(claim(
        flow,
        ClaimKind::TestCoverage,
        text,
        strength,
        Vec::new(),
        "test-coverage",
    ))
}

/// Lift filename-matching tokens from a flow's entity list. We strip
/// the class prefix (`Queue.setBudget` → `setbudget` plus the leading
/// `queue`) and lowercase — filenames are typically `budget.test.ts`,
/// `queue.test.ts`, etc.
fn entity_name_tokens(entities: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut seen: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    for e in entities {
        let lower = e.to_ascii_lowercase();
        // Full qualified name — mostly too specific for a filename
        // match, but try anyway.
        for part in lower.split(['.', ':', ' ']) {
            if part.len() < 3 {
                continue;
            }
            if seen.insert(part.to_string()) {
                out.push(part.to_string());
            }
        }
        // Also emit common verb+noun splits (camelCase → tokens).
        for part in split_camel(&lower) {
            if part.len() < 3 {
                continue;
            }
            if seen.insert(part.clone()) {
                out.push(part);
            }
        }
    }
    out
}

/// Split a camelCase / PascalCase identifier into lowercase parts.
fn split_camel(s: &str) -> Vec<String> {
    let mut parts: Vec<String> = Vec::new();
    let mut current = String::new();
    for ch in s.chars() {
        if ch.is_ascii_uppercase() && !current.is_empty() {
            parts.push(current.clone());
            current.clear();
        }
        for c in ch.to_lowercase() {
            current.push(c);
        }
    }
    if !current.is_empty() {
        parts.push(current);
    }
    parts
}

/// Matches repo-root `examples/` and common variants (`example/`,
/// `demos/`, `demo/`). Like `is_test_path` these are whole-segment
/// matches so we don't flag a file named `examples.ts`.
fn is_example_path(p: &str) -> bool {
    let lower = p.to_ascii_lowercase();
    has_segment(&lower, "examples")
        || has_segment(&lower, "example")
        || has_segment(&lower, "demos")
        || has_segment(&lower, "demo")
}

/* -------------------------------------------------------------------------- */
/* Helpers                                                                    */
/* -------------------------------------------------------------------------- */

fn files_of_flow(artifact: &Artifact, flow: &Flow) -> std::collections::BTreeSet<String> {
    let mut out = std::collections::BTreeSet::new();
    for hid in &flow.hunk_ids {
        for f in files_of_hunk(artifact, hid) {
            out.insert(f);
        }
    }
    out
}

fn files_of_hunk(artifact: &Artifact, hunk_id: &str) -> Vec<String> {
    let Some(h) = artifact.hunks.iter().find(|x| x.id == hunk_id) else {
        return Vec::new();
    };
    let mut files = std::collections::HashSet::<String>::new();
    match &h.kind {
        HunkKind::Call {
            added_edges,
            removed_edges,
        } => {
            for eid in added_edges {
                if let Some(e) = artifact.head.edges.iter().find(|e| &e.id == eid) {
                    if let Some(n) = artifact.head.nodes.iter().find(|n| n.id == e.from) {
                        files.insert(n.file.clone());
                    }
                }
            }
            for eid in removed_edges {
                if let Some(e) = artifact.base.edges.iter().find(|e| &e.id == eid) {
                    if let Some(n) = artifact.base.nodes.iter().find(|n| n.id == e.from) {
                        files.insert(n.file.clone());
                    }
                }
            }
        }
        HunkKind::State { node, .. } | HunkKind::Api { node, .. } => {
            if let Some(n) = artifact.head.nodes.iter().find(|n| n.id == *node) {
                files.insert(n.file.clone());
            }
            if let Some(n) = artifact.base.nodes.iter().find(|n| n.id == *node) {
                files.insert(n.file.clone());
            }
        }
    }
    files.into_iter().collect()
}

fn qname(graph: &adr_core::Graph, id: adr_core::NodeId) -> Option<String> {
    let n = graph.nodes.iter().find(|n| n.id == id)?;
    use adr_core::NodeKind;
    Some(match &n.kind {
        NodeKind::Function { name, .. }
        | NodeKind::Type { name }
        | NodeKind::State { name, .. } => name.clone(),
        NodeKind::ApiEndpoint { method, path } => format!("{method} {path}"),
        NodeKind::File { path } => path.clone(),
    })
}

fn is_test_path(p: &str) -> bool {
    let lower = p.to_ascii_lowercase();
    // Match standard filename-shaped signals (`*.test.ts`, `*.spec.js`)
    // and directory-shaped signals. For directory matches we accept the
    // `tests/` / `spec/` / `__tests__/` segment anywhere in the path —
    // including as the leading segment (glide-mq uses `tests/*.test.ts`
    // at the repo root, no leading slash).
    lower.contains(".test.")
        || lower.contains(".spec.")
        || has_segment(&lower, "tests")
        || has_segment(&lower, "__tests__")
        || has_segment(&lower, "spec")
}

/// True iff `p` contains `seg` as a whole path component — so
/// `has_segment("tests/x.ts", "tests")` matches, but `contests/x.ts`
/// does not. Accepts forward or back slashes.
fn has_segment(p: &str, seg: &str) -> bool {
    p.split(['/', '\\']).any(|part| part == seg)
}

fn claim(
    flow: &Flow,
    kind: ClaimKind,
    text: String,
    strength: Strength,
    entities: Vec<String>,
    collector: &str,
) -> Claim {
    // Stable id across runs: blake3 of (flow_id || kind || text).
    let mut hasher = blake3::Hasher::new();
    hasher.update(flow.id.as_bytes());
    hasher.update(b"|");
    hasher.update(format!("{kind:?}").as_bytes());
    hasher.update(b"|");
    hasher.update(text.as_bytes());
    let id = format!("claim-{}", hasher.finalize().to_hex());
    Claim {
        id,
        text,
        kind,
        strength,
        entities,
        provenance: Provenance::new(
            format!("adr-evidence/{collector}"),
            COLLECTOR_VERSION,
            format!("flow:{}", flow.id),
            flow.id.as_bytes(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adr_core::artifact::PrRef;
    use adr_core::flow::FlowSource;
    use adr_core::graph::{Edge, EdgeId, EdgeKind, Node, NodeId, NodeKind, Span};
    use adr_core::hunks::Hunk;

    fn prov() -> Provenance {
        Provenance::new("test", "0", "t", b"")
    }

    fn fn_node(id: u32, qname: &str, file: &str) -> Node {
        Node {
            id: NodeId(id),
            kind: NodeKind::Function {
                name: qname.into(),
                signature: format!("{qname}()"),
            },
            file: file.into(),
            span: Span { start: 0, end: 1 },
            provenance: prov(),
        }
    }

    fn api_hunk(hid: &str, node_id: u32, before: &str, after: &str) -> Hunk {
        Hunk {
            id: hid.into(),
            kind: HunkKind::Api {
                node: NodeId(node_id),
                before_signature: Some(before.into()),
                after_signature: Some(after.into()),
            },
            provenance: prov(),
        }
    }

    fn call_hunk(hid: &str, added: &[u32]) -> Hunk {
        Hunk {
            id: hid.into(),
            kind: HunkKind::Call {
                added_edges: added.iter().copied().map(EdgeId).collect(),
                removed_edges: Vec::new(),
            },
            provenance: prov(),
        }
    }

    fn edge(id: u32, from: u32, to: u32) -> Edge {
        Edge {
            id: EdgeId(id),
            from: NodeId(from),
            to: NodeId(to),
            kind: EdgeKind::Calls,
            provenance: prov(),
        }
    }

    fn flow_with(
        id: &str,
        hunk_ids: &[&str],
        entities: &[&str],
    ) -> Flow {
        Flow {
            id: id.into(),
            name: format!("<structural: {id}>"),
            rationale: "t".into(),
            source: FlowSource::Structural,
            hunk_ids: hunk_ids.iter().map(|s| s.to_string()).collect(),
            entities: entities.iter().map(|s| s.to_string()).collect(),
            extra_entities: Vec::new(),
            propagation_edges: Vec::new(),
            order: 0,
            evidence: Vec::new(),
            cost: None,
            intent_fit: None,
            proof: None,
        }
    }

    fn seed() -> Artifact {
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.head.nodes = vec![
            fn_node(1, "Queue.enqueue", "src/queue.ts"),
            fn_node(2, "Queue.dequeue", "src/queue.ts"),
            fn_node(3, "formatBudget", "src/budget.ts"),
        ];
        a.base.nodes = a.head.nodes.clone();
        a
    }

    #[test]
    fn single_file_scope_emits_high_claim() {
        let mut a = seed();
        a.hunks = vec![
            api_hunk("h1", 1, "old1", "new1"),
            api_hunk("h2", 2, "old2", "new2"),
        ];
        let flow = flow_with("f1", &["h1", "h2"], &["Queue.enqueue", "Queue.dequeue"]);
        a.flows = vec![flow];
        let out = collect(a);
        let file_claims: Vec<&Claim> = out.flows[0]
            .evidence
            .iter()
            .filter(|c| c.kind == ClaimKind::SingleFile)
            .collect();
        assert_eq!(file_claims.len(), 1);
        assert!(matches!(file_claims[0].strength, Strength::High));
    }

    #[test]
    fn multi_file_scope_downgrades_strength() {
        let mut a = seed();
        a.hunks = vec![
            api_hunk("h1", 1, "old", "new"),
            api_hunk("h2", 3, "old", "new"),
        ];
        a.flows = vec![flow_with("f1", &["h1", "h2"], &["Queue.enqueue", "formatBudget"])];
        let out = collect(a);
        let cross: Vec<&Claim> = out.flows[0]
            .evidence
            .iter()
            .filter(|c| c.kind == ClaimKind::CrossFile)
            .collect();
        assert_eq!(cross.len(), 1);
        assert!(matches!(cross[0].strength, Strength::Medium));
    }

    #[test]
    fn signature_consistency_detects_identical_renames() {
        let mut a = seed();
        a.hunks = vec![
            api_hunk("h1", 1, "(n: number): void", "(n: number): Promise<void>"),
            api_hunk("h2", 2, "(n: number): void", "(n: number): Promise<void>"),
        ];
        a.flows = vec![flow_with("f1", &["h1", "h2"], &["Queue.enqueue", "Queue.dequeue"])];
        let out = collect(a);
        let sig: Vec<&Claim> = out.flows[0]
            .evidence
            .iter()
            .filter(|c| c.kind == ClaimKind::SignatureConsistency)
            .collect();
        assert!(!sig.is_empty(), "expected a SignatureConsistency claim");
    }

    #[test]
    fn call_chain_connected_edges_emit_claim() {
        let mut a = seed();
        // Two call edges sharing Queue.enqueue as an endpoint → connected.
        a.head.edges = vec![edge(10, 3, 1), edge(11, 1, 2)];
        a.hunks = vec![call_hunk("h1", &[10, 11])];
        a.flows = vec![flow_with(
            "f1",
            &["h1"],
            &["formatBudget", "Queue.enqueue", "Queue.dequeue"],
        )];
        let out = collect(a);
        let chain: Vec<&Claim> = out.flows[0]
            .evidence
            .iter()
            .filter(|c| c.kind == ClaimKind::CallChain)
            .collect();
        assert!(!chain.is_empty(), "connected call edges should emit a CallChain claim");
    }

    /// Claim IDs are stable over the (flow_id, kind, text) tuple so the
    /// frontend can key rows without positions drifting across re-runs.
    #[test]
    fn claim_ids_are_deterministic() {
        let mut a = seed();
        a.hunks = vec![api_hunk("h1", 1, "old", "new")];
        a.flows = vec![flow_with("f1", &["h1"], &["Queue.enqueue"])];
        let out1 = collect(a.clone());
        let out2 = collect(a);
        let ids1: Vec<&str> = out1.flows[0].evidence.iter().map(|c| c.id.as_str()).collect();
        let ids2: Vec<&str> = out2.flows[0].evidence.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids1, ids2);
    }

    #[test]
    fn path_classifiers_catch_common_conventions() {
        assert!(is_test_path("src/__tests__/foo.test.ts"));
        assert!(is_test_path("tests/integration/queue.test.ts"));
        assert!(is_test_path("src/queue.spec.ts"));
        assert!(!is_test_path("src/queue.ts"));
        assert!(is_example_path("examples/stream-backpressure.ts"));
        assert!(!is_example_path("src/queue.ts"));
    }

    /// NOTE: per `feedback_proof_not_tests.md` + `project_cost_model.md`,
    /// test-coverage claims are supposed to be a "weak Medium context
    /// signal, never primary proof". The current collector still emits
    /// Strength::High on a name-match. This test captures the current
    /// behaviour so a deliberate strength-downgrade shows up as a diff
    /// rather than silently changing. See `test_coverage_claim` in
    /// lib.rs if you need to revisit the rule.
    #[test]
    fn test_coverage_name_match_currently_emits_high() {
        let mut a = seed();
        // Add a name-matching test file into the graph.
        a.head.nodes.push(fn_node(10, "testQueue", "tests/queue.test.ts"));
        a.hunks = vec![api_hunk("h1", 1, "old", "new")];
        a.flows = vec![flow_with("f1", &["h1"], &["Queue.enqueue"])];
        let out = collect(a);
        let tc: Option<&Claim> = out.flows[0]
            .evidence
            .iter()
            .find(|c| c.kind == ClaimKind::TestCoverage);
        let tc = tc.expect("expected a TestCoverage claim");
        // Current behaviour: name-match → High. Change this assertion
        // if you intentionally downgrade the collector per the memo.
        assert!(
            matches!(tc.strength, Strength::High),
            "TestCoverage strength changed from High — if this was on purpose, \
             update this assertion + the feedback_proof_not_tests.md memory."
        );
    }

    #[test]
    fn no_tests_in_repo_emits_low_strength() {
        let mut a = seed();
        a.hunks = vec![api_hunk("h1", 1, "old", "new")];
        a.flows = vec![flow_with("f1", &["h1"], &["Queue.enqueue"])];
        let out = collect(a);
        let tc = out.flows[0]
            .evidence
            .iter()
            .find(|c| c.kind == ClaimKind::TestCoverage)
            .expect("expected a TestCoverage claim even when no tests exist");
        assert!(matches!(tc.strength, Strength::Low));
    }

    #[test]
    fn empty_flow_produces_no_evidence() {
        let mut a = seed();
        a.flows = vec![flow_with("f1", &[], &[])];
        let out = collect(a);
        assert!(out.flows[0].evidence.is_empty());
    }
}
