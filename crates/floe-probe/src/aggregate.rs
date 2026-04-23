//! Fold per-probe [`ProbeResult`]s into one [`AggregateBaseline`] with
//! per-entity cost scores. Weights are frozen at the probe-set version;
//! any change bumps `PROBE_SET_VERSION`.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::probes::ProbeId;
use crate::session::ProbeResult;

/// v0.1 weights — frozen.
const ALPHA_VISITS: f32 = 1.0;
const BETA_TOKENS: f32 = 0.001;
const GAMMA_TURNS: f32 = 2.0;

/// Per-entity cost observation folded across all probes.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EntityCost {
    /// Unweighted total visits across every probe session.
    pub visits: u32,
    /// Approximate tokens spent while this entity was in scope.
    /// Approximation: we don't track token-per-message precisely; we
    /// attribute `tokens_out / tool_calls_total` to each tool call the
    /// entity showed up in. Good enough for a relative scale.
    pub tokens: u32,
    /// Number of distinct probe sessions the entity appeared in.
    pub sessions_present: u32,
    /// Weighted cost — the scalar the per-flow delta math uses.
    pub cost: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateTotals {
    pub entities: u32,
    pub tokens: u32,
    pub tool_calls: u32,
    pub turns: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AggregateBaseline {
    pub schema_version: String,
    pub probe_set_version: String,
    pub probe_model: String,
    pub per_entity: HashMap<String, EntityCost>,
    /// Per-probe per-entity cost — used by `floe-cost` to attribute a
    /// flow's delta onto the correct signed axis. Keyed by the string
    /// form of `ProbeId` (enum keys in JSON maps are clumsy).
    #[serde(default)]
    pub per_probe_entity_cost: HashMap<String, HashMap<String, f32>>,
    pub per_probe: HashMap<ProbeId, ProbeResultSummary>,
    pub totals: AggregateTotals,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProbeResultSummary {
    pub turns: u32,
    pub tool_calls: u32,
    pub tokens_in: u32,
    pub tokens_out: u32,
    pub duration_ms: u64,
    pub end_reason: String,
}

pub fn aggregate(probe_model: &str, results: &[ProbeResult]) -> AggregateBaseline {
    let mut per_entity: HashMap<String, EntityCost> = HashMap::new();
    let mut per_probe: HashMap<ProbeId, ProbeResultSummary> = HashMap::new();
    let mut per_probe_entity_cost: HashMap<String, HashMap<String, f32>> = HashMap::new();
    let mut t_tokens: u32 = 0;
    let mut t_calls: u32 = 0;
    let mut t_turns: u32 = 0;
    let mut t_duration: u64 = 0;

    for r in results {
        let total_entities_in_probe: u32 = r.per_entity_visits.values().sum();
        let tokens_per_visit = if total_entities_in_probe == 0 {
            0.0
        } else {
            r.tokens_out as f32 / total_entities_in_probe as f32
        };

        let mut probe_entity_cost: HashMap<String, f32> = HashMap::new();
        for (entity, visits) in &r.per_entity_visits {
            let entity_tokens = (tokens_per_visit * (*visits as f32)).round() as u32;

            let entry = per_entity.entry(entity.clone()).or_insert(EntityCost {
                visits: 0,
                tokens: 0,
                sessions_present: 0,
                cost: 0.0,
            });
            entry.visits += visits;
            entry.tokens += entity_tokens;
            entry.sessions_present += 1;

            // Per-probe contribution under the same weight formula —
            // each probe contributes 1 "session_present" on its own.
            let probe_cost = ALPHA_VISITS * (*visits as f32)
                + BETA_TOKENS * (entity_tokens as f32)
                + GAMMA_TURNS * 1.0;
            probe_entity_cost.insert(entity.clone(), probe_cost);
        }
        per_probe_entity_cost.insert(r.probe_id.as_str().to_string(), probe_entity_cost);

        per_probe.insert(
            r.probe_id,
            ProbeResultSummary {
                turns: r.turns,
                tool_calls: r.tool_calls,
                tokens_in: r.tokens_in,
                tokens_out: r.tokens_out,
                duration_ms: r.duration_ms,
                end_reason: r.end_reason.clone(),
            },
        );

        t_tokens += r.tokens_in + r.tokens_out;
        t_calls += r.tool_calls;
        t_turns += r.turns;
        t_duration += r.duration_ms;
    }

    // Apply the weighted cost formula now that all probes are folded.
    for entry in per_entity.values_mut() {
        entry.cost = ALPHA_VISITS * (entry.visits as f32)
            + BETA_TOKENS * (entry.tokens as f32)
            + GAMMA_TURNS * (entry.sessions_present as f32);
    }

    AggregateBaseline {
        schema_version: "0.1.0".into(),
        probe_set_version: super::PROBE_SET_VERSION.into(),
        probe_model: probe_model.into(),
        totals: AggregateTotals {
            entities: per_entity.len() as u32,
            tokens: t_tokens,
            tool_calls: t_calls,
            turns: t_turns,
            duration_ms: t_duration,
        },
        per_entity,
        per_probe_entity_cost,
        per_probe,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mk_probe(id: ProbeId, visits: &[(&str, u32)], tokens: u32) -> ProbeResult {
        ProbeResult {
            probe_id: id,
            turns: 5,
            tool_calls: visits.iter().map(|(_, v)| *v).sum(),
            tokens_in: 1000,
            tokens_out: tokens,
            duration_ms: 12000,
            per_entity_visits: visits.iter().map(|(e, v)| (e.to_string(), *v)).collect(),
            final_answer: String::new(),
            end_reason: "completed".into(),
        }
    }

    #[test]
    fn aggregate_folds_three_probes() {
        let r1 = mk_probe(ProbeId::ApiSurface, &[("Queue.setBudget", 3), ("Job.stream", 1)], 500);
        let r2 = mk_probe(ProbeId::ExternalBoundaries, &[("Queue.setBudget", 1)], 300);
        let r3 = mk_probe(ProbeId::TypeCallsites, &[("Job.stream", 2), ("Job.streamChunk", 4)], 800);

        let agg = aggregate("qwen3.5:27b-q4_K_M", &[r1, r2, r3]);
        assert_eq!(agg.totals.entities, 3);

        let setbudget = agg.per_entity.get("Queue.setBudget").unwrap();
        assert_eq!(setbudget.visits, 4);
        assert_eq!(setbudget.sessions_present, 2);
        assert!(setbudget.cost > 0.0);

        let streamchunk = agg.per_entity.get("Job.streamChunk").unwrap();
        assert_eq!(streamchunk.visits, 4);
        assert_eq!(streamchunk.sessions_present, 1);
        assert!(streamchunk.cost > setbudget.cost - 6.0);
    }
}
