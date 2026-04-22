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

    // Pick strength from whatever's strongest. A test-name match wins
    // over an example-only match; both beat "tests exist in the repo
    // but none name-match"; the truly bad case is "no tests at all".
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
    } else if total_tests > 0 || total_examples > 0 {
        (
            Strength::Medium,
            format!(
                "Repo has {total_tests} test file(s) and {total_examples} example(s) but none name-match this flow — coverage may still be present via a different naming scheme."
            ),
        )
    } else {
        (
            Strength::Low,
            "No test or example files found in the repo — regression risk is high.".into(),
        )
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
