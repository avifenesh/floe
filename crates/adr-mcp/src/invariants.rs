//! Global invariants re-checked at `finalize`. Per-mutation checks live in
//! [`super::handlers`]; these catch drift the per-mutation checks can't
//! (e.g. a hunk left uncovered after a series of removes).

use crate::handlers::stamp_accepted;
use crate::state::{Session, RESERVED_NAMES};
use crate::wire::FinalizeOutcome;

/// The per-run tool-call cap. Mirrors [`crate::state::DEFAULT_CALL_BUDGET`]
/// but is enforced on `finalize` regardless of what the session was
/// instantiated with — you can't opt out of the contract budget.
pub const CONTRACT_CALL_CAP: u32 = 200;

/// Run all four invariants and either promote the working set to accepted
/// or return a structured reject reason.
pub fn finalize(s: &mut Session, model: &str, version: &str) -> FinalizeOutcome {
    // Rule 1: every hunk in the artifact appears in ≥ 1 flow.
    let covered: std::collections::HashSet<&String> = s
        .working_flows()
        .iter()
        .flat_map(|f| f.hunk_ids.iter())
        .collect();
    let missing: Vec<&str> = s
        .artifact()
        .hunks
        .iter()
        .filter(|h| !covered.contains(&h.id))
        .map(|h| h.id.as_str())
        .collect();
    if !missing.is_empty() {
        return reject(
            "coverage",
            format!(
                "{} hunk(s) are not assigned to any flow: {}",
                missing.len(),
                missing.iter().take(3).copied().collect::<Vec<_>>().join(", ")
            ),
        );
    }

    // Rule 2: no reserved names.
    for f in s.working_flows() {
        let lower = f.name.to_ascii_lowercase();
        if RESERVED_NAMES.iter().any(|r| *r == lower) {
            return reject(
                "reserved-name",
                format!("flow `{}` uses the reserved name `{}`", f.id, f.name),
            );
        }
    }

    // Rule 3: every referenced entity exists on either side of the graph.
    for f in s.working_flows() {
        for e in f.entities.iter().chain(f.extra_entities.iter()) {
            if !crate::handlers::has_entity(s, e) {
                return reject(
                    "entity-not-found",
                    format!("flow `{}` references unknown entity `{}`", f.id, e),
                );
            }
        }
        for hid in &f.hunk_ids {
            if !crate::handlers::has_hunk(s, hid) {
                return reject(
                    "hunk-not-found",
                    format!("flow `{}` references unknown hunk `{}`", f.id, hid),
                );
            }
        }
    }

    // Rule 4: tool-call cap never exceeded. (Per-call check already guards
    // against over-spend, but finalize reasserts so a future session
    // override can't silently widen the cap.)
    if s.call_count() > CONTRACT_CALL_CAP {
        return reject(
            "call-budget",
            format!(
                "used {} of {} allowed tool calls",
                s.call_count(),
                CONTRACT_CALL_CAP
            ),
        );
    }

    let flows = stamp_accepted(s, model, version);
    FinalizeOutcome::Accepted { flows }
}

fn reject(rule: &'static str, detail: String) -> FinalizeOutcome {
    FinalizeOutcome::Rejected {
        rejected_rule: rule.into(),
        detail,
    }
}
