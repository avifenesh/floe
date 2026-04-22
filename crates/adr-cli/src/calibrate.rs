//! Flow-set comparison — the calibration primitive.
//!
//! Given two artifacts for the same PR (e.g. one run with Qwen 3.5 local
//! and one with GLM-4.7 cloud), produce a report that answers:
//!
//! - How many flows did each run produce?
//! - Which A-flow corresponds most closely to which B-flow, by hunk
//!   overlap (Jaccard)?
//! - Which flows have no strong match on the other side (orphans)?
//! - How does per-flow cost compare across the matched pairs?
//!
//! Flow IDs differ across LLM runs (they're minted per-session) so the
//! comparison is purely on hunk_id sets — which hunks end up together.
//! Name drift across runs is what we want to surface, not filter out.

use std::collections::BTreeSet;

use adr_core::{Artifact, Flow};
use serde::Serialize;

/// A single A-to-B pairing with overlap metrics.
#[derive(Debug, Clone, Serialize)]
pub struct FlowPairing {
    pub a_name: String,
    pub b_name: String,
    pub jaccard: f32,
    pub shared_hunks: usize,
    pub a_only: usize,
    pub b_only: usize,
    /// `(a_cost, b_cost)` when both flows carry a cost estimate.
    /// Signed — see [`adr_core::Cost`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost_pair: Option<(i32, i32)>,
}

/// Flow from one side with no strong match on the other — threshold is
/// Jaccard < 0.15.
#[derive(Debug, Clone, Serialize)]
pub struct Orphan {
    pub name: String,
    pub hunk_count: usize,
    pub best_match: Option<String>,
    pub best_jaccard: f32,
}

#[derive(Debug, Clone, Serialize)]
pub struct CalibrationReport {
    pub a_flow_count: usize,
    pub b_flow_count: usize,
    /// Fraction of hunks that land in "equivalent" flows on both sides.
    /// Defined as: for each hunk in A, does any A-flow containing it
    /// pair (Jaccard ≥ 0.5) with a B-flow that also contains it?
    pub hunk_agreement: f32,
    pub pairs: Vec<FlowPairing>,
    pub a_orphans: Vec<Orphan>,
    pub b_orphans: Vec<Orphan>,
}

pub fn compare(a: &Artifact, b: &Artifact) -> CalibrationReport {
    let pairs = greedy_pair(&a.flows, &b.flows);
    let a_orphans = orphans(&a.flows, &b.flows);
    let b_orphans = orphans(&b.flows, &a.flows);
    let hunk_agreement = compute_hunk_agreement(&a.flows, &b.flows);

    CalibrationReport {
        a_flow_count: a.flows.len(),
        b_flow_count: b.flows.len(),
        hunk_agreement,
        pairs,
        a_orphans,
        b_orphans,
    }
}

/// Greedy Hungarian-ish: iterate A flows in descending hunk-count order;
/// for each, pick the unclaimed B-flow with highest Jaccard overlap.
fn greedy_pair(a: &[Flow], b: &[Flow]) -> Vec<FlowPairing> {
    let mut a_sorted: Vec<&Flow> = a.iter().collect();
    a_sorted.sort_by_key(|f| std::cmp::Reverse(f.hunk_ids.len()));
    let mut claimed: BTreeSet<&str> = BTreeSet::new();
    let mut out = Vec::new();
    for fa in a_sorted {
        let mut best: Option<(&Flow, f32, usize, usize, usize)> = None;
        for fb in b {
            if claimed.contains(fb.id.as_str()) {
                continue;
            }
            let (j, shared, a_only, b_only) = jaccard(&fa.hunk_ids, &fb.hunk_ids);
            if best.map(|(_, bj, ..)| j > bj).unwrap_or(true) {
                best = Some((fb, j, shared, a_only, b_only));
            }
        }
        if let Some((fb, j, shared, a_only, b_only)) = best {
            claimed.insert(fb.id.as_str());
            out.push(FlowPairing {
                a_name: fa.name.clone(),
                b_name: fb.name.clone(),
                jaccard: j,
                shared_hunks: shared,
                a_only,
                b_only,
                cost_pair: match (&fa.cost, &fb.cost) {
                    (Some(ca), Some(cb)) => Some((ca.net, cb.net)),
                    _ => None,
                },
            });
        }
    }
    out
}

fn orphans(primary: &[Flow], other: &[Flow]) -> Vec<Orphan> {
    primary
        .iter()
        .filter_map(|fp| {
            let (best_name, best_j) = other
                .iter()
                .map(|fo| (fo.name.clone(), jaccard(&fp.hunk_ids, &fo.hunk_ids).0))
                .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
                .unwrap_or_default();
            if best_j < 0.15 {
                Some(Orphan {
                    name: fp.name.clone(),
                    hunk_count: fp.hunk_ids.len(),
                    best_match: if best_name.is_empty() { None } else { Some(best_name) },
                    best_jaccard: best_j,
                })
            } else {
                None
            }
        })
        .collect()
}

fn jaccard(a: &[String], b: &[String]) -> (f32, usize, usize, usize) {
    let sa: BTreeSet<&str> = a.iter().map(String::as_str).collect();
    let sb: BTreeSet<&str> = b.iter().map(String::as_str).collect();
    let shared = sa.intersection(&sb).count();
    let union = sa.union(&sb).count();
    let j = if union == 0 { 0.0 } else { shared as f32 / union as f32 };
    let a_only = sa.difference(&sb).count();
    let b_only = sb.difference(&sa).count();
    (j, shared, a_only, b_only)
}

fn compute_hunk_agreement(a: &[Flow], b: &[Flow]) -> f32 {
    let all_hunks: BTreeSet<&str> = a
        .iter()
        .chain(b.iter())
        .flat_map(|f| f.hunk_ids.iter().map(String::as_str))
        .collect();
    if all_hunks.is_empty() {
        return 1.0;
    }
    let mut agree = 0usize;
    for h in &all_hunks {
        let a_flows: Vec<&Flow> = a.iter().filter(|f| f.hunk_ids.iter().any(|x| x == h)).collect();
        let b_flows: Vec<&Flow> = b.iter().filter(|f| f.hunk_ids.iter().any(|x| x == h)).collect();
        if a_flows.is_empty() || b_flows.is_empty() {
            continue;
        }
        // Paired: any pair of (A-flow, B-flow) containing this hunk that
        // also has Jaccard ≥ 0.5 on their hunk sets.
        for fa in &a_flows {
            for fb in &b_flows {
                if jaccard(&fa.hunk_ids, &fb.hunk_ids).0 >= 0.5 {
                    agree += 1;
                    break;
                }
            }
            if agree > 0 && std::ptr::eq(*a_flows.last().unwrap(), *fa) {
                break;
            }
        }
    }
    agree as f32 / all_hunks.len() as f32
}

/// Render a short human-readable text report. JSON is produced via serde
/// on `CalibrationReport` directly.
pub fn format_text(r: &CalibrationReport) -> String {
    let mut s = String::new();
    s.push_str(&format!(
        "flows         A={}  B={}\n",
        r.a_flow_count, r.b_flow_count
    ));
    s.push_str(&format!(
        "hunk agreement  {:.1}%\n\n",
        r.hunk_agreement * 100.0
    ));
    s.push_str("pairs:\n");
    for p in &r.pairs {
        let cost = match p.cost_pair {
            Some((a, b)) => format!("  cost A={a} B={b}"),
            None => String::new(),
        };
        s.push_str(&format!(
            "  {:>5.1}%  A:{:<40}  B:{:<40}  shared={} Δ=+{}/-{}{}\n",
            p.jaccard * 100.0,
            truncate(&p.a_name, 40),
            truncate(&p.b_name, 40),
            p.shared_hunks,
            p.b_only,
            p.a_only,
            cost,
        ));
    }
    if !r.a_orphans.is_empty() {
        s.push_str("\nA-only (no strong match in B):\n");
        for o in &r.a_orphans {
            let bm = o.best_match.as_deref().unwrap_or("—");
            s.push_str(&format!(
                "  {:<40}  {} hunks  closest B: {} ({:.0}%)\n",
                truncate(&o.name, 40),
                o.hunk_count,
                truncate(bm, 30),
                o.best_jaccard * 100.0,
            ));
        }
    }
    if !r.b_orphans.is_empty() {
        s.push_str("\nB-only (no strong match in A):\n");
        for o in &r.b_orphans {
            let am = o.best_match.as_deref().unwrap_or("—");
            s.push_str(&format!(
                "  {:<40}  {} hunks  closest A: {} ({:.0}%)\n",
                truncate(&o.name, 40),
                o.hunk_count,
                truncate(am, 30),
                o.best_jaccard * 100.0,
            ));
        }
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use adr_core::{artifact::PrRef, FlowSource};

    fn flow(id: &str, name: &str, hunks: &[&str]) -> Flow {
        Flow {
            id: id.into(),
            name: name.into(),
            rationale: String::new(),
            source: FlowSource::Structural,
            hunk_ids: hunks.iter().map(|s| s.to_string()).collect(),
            entities: Vec::new(),
            extra_entities: Vec::new(),
            propagation_edges: Vec::new(),
            order: 0,
            evidence: Vec::new(),
            cost: None,
            intent_fit: None,
            proof: None,
        }
    }

    fn artifact(flows: Vec<Flow>) -> Artifact {
        let mut a = Artifact::new(PrRef {
            repo: "t".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = flows;
        a
    }

    #[test]
    fn identical_runs_agree() {
        let flows = vec![
            flow("f1", "Budget", &["h1", "h2", "h3"]),
            flow("f2", "Streaming", &["h4", "h5"]),
        ];
        let a = artifact(flows.clone());
        let b = artifact(flows);
        let r = compare(&a, &b);
        assert_eq!(r.a_flow_count, 2);
        assert_eq!(r.b_flow_count, 2);
        assert!(r.hunk_agreement > 0.99);
        assert!(r.a_orphans.is_empty());
        assert!(r.b_orphans.is_empty());
    }

    #[test]
    fn rename_only_still_pairs() {
        // Same partition, different names — 100% Jaccard match but the
        // names differ. Calibration should surface the rename.
        let a = artifact(vec![flow("f1", "Budget tracking", &["h1", "h2"])]);
        let b = artifact(vec![flow("x1", "Flow budget support", &["h1", "h2"])]);
        let r = compare(&a, &b);
        assert_eq!(r.pairs.len(), 1);
        assert!((r.pairs[0].jaccard - 1.0).abs() < 0.01);
        assert_eq!(r.pairs[0].a_name, "Budget tracking");
        assert_eq!(r.pairs[0].b_name, "Flow budget support");
    }

    #[test]
    fn disjoint_flows_are_orphans() {
        let a = artifact(vec![flow("f1", "Queue budget", &["h1", "h2"])]);
        let b = artifact(vec![flow("x1", "Streaming", &["h3", "h4"])]);
        let r = compare(&a, &b);
        assert_eq!(r.a_orphans.len(), 1);
        assert_eq!(r.b_orphans.len(), 1);
    }

    #[test]
    fn partial_overlap_pairs_but_flags() {
        // A has one flow of 4 hunks; B splits into two flows of 2 each.
        let a = artifact(vec![flow("f1", "Budget", &["h1", "h2", "h3", "h4"])]);
        let b = artifact(vec![
            flow("x1", "Queue budget", &["h1", "h2"]),
            flow("x2", "TestQueue budget", &["h3", "h4"]),
        ]);
        let r = compare(&a, &b);
        // A's single flow pairs with one of B's; the other B-flow is orphan.
        assert_eq!(r.pairs.len(), 1);
        assert_eq!(r.b_orphans.len(), 1);
    }
}
