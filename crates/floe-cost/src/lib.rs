//! Per-flow signed cost from probe baselines.
//!
//! Contract (supersedes the v0 heuristic stub):
//!
//! - Takes two [`AggregateBaseline`]s (base + head) for the artifact's
//!   pinned probe model.
//! - For every entity in the flow, computes per-probe delta:
//!   `delta[probe, entity] = head[probe, entity] - base[probe, entity]`.
//! - Attributes each probe's delta onto one of the three signed
//!   [`Axes`] per the RFC mapping (api-surface → continuation,
//!   external-boundaries → operational, type-callsites → runtime).
//! - Emits drivers with sample entities + human labels so the reviewer
//!   can see *why* the flow moved.
//!
//! Negative numbers mean the refactor made the flow easier for the next
//! LLM session to navigate; positive means harder. Baseline units are
//! arbitrary (see `docs/scope-5-cost-model.md`).

use std::collections::HashMap;

use floe_core::{
    artifact::ArtifactBaseline,
    evidence::{Axes, Cost, CostDriver},
    Artifact, Flow, FlowSource,
};
use floe_probe::{AggregateBaseline, ProbeId};

/// Attach a [`Cost`] to every flow in the artifact using the supplied
/// probe baselines. Idempotent; overwrites any existing `flow.cost`.
pub fn attribute_from_baselines(
    mut artifact: Artifact,
    base: &AggregateBaseline,
    head: &AggregateBaseline,
) -> Artifact {
    let mut flows = std::mem::take(&mut artifact.flows);
    for flow in flows.iter_mut() {
        flow.cost = Some(score_flow(flow, base, head));
    }
    artifact.baseline = Some(baseline_summary(base, head, &flows));
    artifact.flows = flows;
    artifact
}

/// Build the repo-wide `ArtifactBaseline` — the denominators the frontend
/// uses for %-of-baseline bars, plus the model pin RFC v0.3 §9 requires.
/// Per-axis totals are summed from the base run's `per_probe_entity_cost`;
/// token totals come straight from `totals.tokens`. Synthesis + proof
/// models are plucked from the flows themselves — any `FlowSource::Llm`
/// flow gives us the synthesis model, any `flow.proof` gives us the
/// proof model. They're `None` when the corresponding pass was skipped.
fn baseline_summary(
    base: &AggregateBaseline,
    head: &AggregateBaseline,
    flows: &[Flow],
) -> ArtifactBaseline {
    let continuation = probe_axis_total(base, ProbeId::ApiSurface);
    let operational = probe_axis_total(base, ProbeId::ExternalBoundaries);
    let runtime = probe_axis_total(base, ProbeId::TypeCallsites);
    let synthesis_model = flows.iter().find_map(|f| match &f.source {
        FlowSource::Llm { model, .. } => Some(model.clone()),
        FlowSource::Structural => None,
    });
    let proof_model = flows
        .iter()
        .find_map(|f| f.proof.as_ref().map(|p| p.model.clone()));
    ArtifactBaseline {
        axes_base: Axes {
            continuation,
            runtime,
            operational,
        },
        tokens_base: base.totals.tokens,
        tokens_head: head.totals.tokens,
        probe_model: base.probe_model.clone(),
        probe_set_version: base.probe_set_version.clone(),
        synthesis_model,
        proof_model,
    }
}

fn probe_axis_total(baseline: &AggregateBaseline, probe: ProbeId) -> i32 {
    baseline
        .per_probe_entity_cost
        .get(probe.as_str())
        .map(|m| m.values().sum::<f32>().round() as i32)
        .unwrap_or(0)
}

fn score_flow(flow: &Flow, base: &AggregateBaseline, head: &AggregateBaseline) -> Cost {
    // Union of entities referenced by the flow — flow.entities covers
    // the directly-touched ones; extra_entities covers LLM-added
    // context. Score both since reviewers care about total reach.
    let mut seen: HashMap<&str, ()> = HashMap::new();
    for e in flow.entities.iter().chain(flow.extra_entities.iter()) {
        seen.insert(e.as_str(), ());
    }

    let mut axes = Axes::default();
    let mut drivers: Vec<CostDriver> = Vec::new();

    for probe in [
        ProbeId::ApiSurface,
        ProbeId::ExternalBoundaries,
        ProbeId::TypeCallsites,
    ] {
        let (delta, sample_entity) = probe_delta(&seen, base, head, probe);
        let delta_int = delta.round() as i32;
        match probe_to_axis(probe) {
            Axis::Continuation => axes.continuation += delta_int,
            Axis::Operational => axes.operational += delta_int,
            Axis::Runtime => axes.runtime += delta_int,
        }
        if delta_int != 0 {
            drivers.push(CostDriver {
                label: probe_label(probe).to_string(),
                value: delta_int,
                detail: detail_line(delta_int, sample_entity.as_deref(), probe),
            });
        }
    }

    let net = axes.continuation + axes.operational + axes.runtime;
    let tokens_delta = flow_tokens_delta(&seen, base, head);
    Cost {
        net,
        axes,
        drivers,
        tokens_delta,
        probe_model: head.probe_model.clone(),
        probe_set_version: head.probe_set_version.clone(),
    }
}

/// Signed token delta attributed to the flow: `Σ (head - base)` over
/// the flow's entities, with the same asymmetric clamp as
/// [`probe_delta`] — shared entities give up their "easier" credit so
/// additive PRs can't appear to reduce token usage just because the
/// probe happened to visit shared symbols less on the busier head run.
fn flow_tokens_delta(
    entities: &HashMap<&str, ()>,
    base: &AggregateBaseline,
    head: &AggregateBaseline,
) -> i32 {
    let mut d: i64 = 0;
    for entity in entities.keys() {
        let b_opt = base.per_entity.get(*entity).map(|e| e.tokens as i64);
        let h_opt = head.per_entity.get(*entity).map(|e| e.tokens as i64);
        let contrib = match (b_opt, h_opt) {
            (Some(b), Some(h)) => (h - b).max(0), // shared: clamp easier→0
            (None, Some(h)) => h,                   // added: positive
            (Some(b), None) => -b,                  // deleted: negative
            (None, None) => 0,
        };
        d += contrib;
    }
    d.clamp(i32::MIN as i64, i32::MAX as i64) as i32
}

enum Axis {
    Continuation,
    Operational,
    Runtime,
}

fn probe_to_axis(p: ProbeId) -> Axis {
    match p {
        ProbeId::ApiSurface => Axis::Continuation,
        ProbeId::ExternalBoundaries => Axis::Operational,
        ProbeId::TypeCallsites => Axis::Runtime,
    }
}

fn probe_label(p: ProbeId) -> &'static str {
    match p {
        ProbeId::ApiSurface => "API-surface navigation",
        ProbeId::ExternalBoundaries => "external-boundary reach",
        ProbeId::TypeCallsites => "type call-site tracing",
    }
}

/// Sum per-entity delta for one probe over the flow's entities.
///
/// **Asymmetric per-entity semantics**, so the invariant "pure
/// additive PRs can't be cheaper to navigate" holds even when probe
/// noise shifts per-entity costs on shared symbols:
///
/// - **Shared** entity (present on both sides): delta clamps at 0.
///   The probe can visit a shared symbol fewer times on head (busier
///   with new code), producing a stochastic negative — but a symbol
///   that didn't change cannot truly have gotten easier to
///   understand. So we give no "easier" credit for shared entities.
/// - **Added** entity (head-only): full positive cost — real new
///   navigation surface.
/// - **Deleted** entity (base-only): full negative — deletions
///   genuinely simplify, which is what cost is meant to reflect.
///
/// Returns the signed delta sum + the qualified name of the entity
/// that moved the most (used for the driver's detail line).
fn probe_delta(
    entities: &HashMap<&str, ()>,
    base: &AggregateBaseline,
    head: &AggregateBaseline,
    probe: ProbeId,
) -> (f32, Option<String>) {
    let probe_key = probe.as_str();
    let base_per = base.per_probe_entity_cost.get(probe_key);
    let head_per = head.per_probe_entity_cost.get(probe_key);
    let mut delta_sum: f32 = 0.0;
    let mut biggest_mover: Option<(String, f32)> = None;
    for entity in entities.keys() {
        let b_opt = base_per.and_then(|m| m.get(*entity)).copied();
        let h_opt = head_per.and_then(|m| m.get(*entity)).copied();
        let (b, h) = (b_opt.unwrap_or(0.0), h_opt.unwrap_or(0.0));
        let raw = h - b;
        // Asymmetric clamp.
        let d = match (b_opt, h_opt) {
            (Some(_), Some(_)) => raw.max(0.0), // shared: clamp easier→0
            (None, Some(_)) => raw,              // added: positive
            (Some(_), None) => raw,              // deleted: negative
            (None, None) => 0.0,                 // should not happen
        };
        if d == 0.0 {
            continue;
        }
        delta_sum += d;
        let abs = d.abs();
        if biggest_mover.as_ref().map_or(true, |(_, m)| abs > *m) {
            biggest_mover = Some((entity.to_string(), abs));
        }
    }
    (delta_sum, biggest_mover.map(|(e, _)| e))
}

fn detail_line(delta: i32, sample: Option<&str>, probe: ProbeId) -> String {
    let direction = if delta < 0 { "easier" } else { "harder" };
    let sample_s = match sample {
        Some(e) => format!(", driven by `{e}`"),
        None => String::new(),
    };
    match probe {
        ProbeId::ApiSurface => format!(
            "{direction} for the next session to map the public API{sample_s}."
        ),
        ProbeId::ExternalBoundaries => format!(
            "{direction} to enumerate the external side-effects{sample_s}."
        ),
        ProbeId::TypeCallsites => format!(
            "{direction} to trace type usage across call-sites{sample_s}."
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use floe_core::artifact::PrRef;
    use floe_core::FlowSource;
    use floe_probe::aggregate::{AggregateTotals, EntityCost};

    fn baseline(probe_costs: &[(ProbeId, &[(&str, f32)])]) -> AggregateBaseline {
        baseline_with_tokens(probe_costs, &[])
    }

    fn baseline_with_tokens(
        probe_costs: &[(ProbeId, &[(&str, f32)])],
        entity_tokens: &[(&str, u32)],
    ) -> AggregateBaseline {
        let mut per_probe_entity_cost: HashMap<String, HashMap<String, f32>> = HashMap::new();
        let mut per_entity: HashMap<String, EntityCost> = HashMap::new();
        for (probe, costs) in probe_costs {
            let mut m = HashMap::new();
            for (e, c) in *costs {
                m.insert(e.to_string(), *c);
                let entry = per_entity.entry(e.to_string()).or_insert(EntityCost {
                    visits: 0,
                    tokens: 0,
                    sessions_present: 0,
                    cost: 0.0,
                });
                entry.cost += *c;
            }
            per_probe_entity_cost.insert(probe.as_str().to_string(), m);
        }
        let mut tokens_total: u32 = 0;
        for (e, t) in entity_tokens {
            let entry = per_entity.entry((*e).into()).or_insert(EntityCost {
                visits: 0,
                tokens: 0,
                sessions_present: 0,
                cost: 0.0,
            });
            entry.tokens = *t;
            tokens_total += t;
        }
        AggregateBaseline {
            schema_version: "0.1.0".into(),
            probe_set_version: "0.1".into(),
            probe_model: "qwen3.5:27b-q4_K_M".into(),
            per_entity,
            per_probe_entity_cost,
            per_probe: HashMap::new(),
            totals: AggregateTotals {
                entities: 0,
                tokens: tokens_total,
                tool_calls: 0,
                turns: 0,
                duration_ms: 0,
            },
        }
    }

    fn flow(entities: &[&str]) -> Flow {
        Flow {
            id: "flow-1".into(),
            name: "test".into(),
            rationale: "".into(),
            source: FlowSource::Structural,
            hunk_ids: Vec::new(),
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

    #[test]
    fn refactor_goes_negative_when_head_cheaper() {
        // Entity `A.heavy` is present on base but *gone* on head — a
        // true deletion. Deleted entities contribute negative delta
        // (the probe doesn't have to visit them any more). Shared-
        // entity noise that might have also tilted negative is now
        // clamped at 0, so the ONLY source of negative cost is
        // genuine removal — which is what we want.
        let base = baseline(&[
            (ProbeId::ApiSurface, &[("A.heavy", 10.0), ("A.other", 4.0)]),
            (ProbeId::ExternalBoundaries, &[("A.heavy", 6.0)]),
            (ProbeId::TypeCallsites, &[("A.heavy", 8.0)]),
        ]);
        let head = baseline(&[
            // A.heavy deleted; A.other unchanged.
            (ProbeId::ApiSurface, &[("A.other", 4.0)]),
            (ProbeId::ExternalBoundaries, &[]),
            (ProbeId::TypeCallsites, &[]),
        ]);
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = vec![flow(&["A.heavy"])];
        let out = attribute_from_baselines(a, &base, &head);
        let c = out.flows[0].cost.as_ref().unwrap();
        assert!(c.net < 0, "deletion should go negative; got {}", c.net);
        assert!(c.axes.continuation < 0);
        assert!(c.axes.operational < 0);
        assert!(c.axes.runtime < 0);
    }

    #[test]
    fn shared_entity_with_noise_does_not_go_negative() {
        // Shared entity whose probe-measured cost drops on head —
        // pure noise from the stochastic probe. Under the new
        // asymmetric clamp this contributes 0, not negative. The
        // invariant "PR that only adds code can't look easier" holds.
        let base = baseline(&[
            (ProbeId::ApiSurface, &[("Shared.thing", 10.0)]),
            (ProbeId::ExternalBoundaries, &[("Shared.thing", 6.0)]),
            (ProbeId::TypeCallsites, &[("Shared.thing", 8.0)]),
        ]);
        let head = baseline(&[
            (ProbeId::ApiSurface, &[("Shared.thing", 3.0)]),
            (ProbeId::ExternalBoundaries, &[("Shared.thing", 2.0)]),
            (ProbeId::TypeCallsites, &[("Shared.thing", 3.0)]),
        ]);
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = vec![flow(&["Shared.thing"])];
        let out = attribute_from_baselines(a, &base, &head);
        let c = out.flows[0].cost.as_ref().unwrap();
        assert_eq!(c.net, 0, "shared-entity drops should clamp to 0, got {}", c.net);
    }

    #[test]
    fn new_entity_counts_as_pure_addition() {
        let base = baseline(&[(ProbeId::ApiSurface, &[])]);
        let head = baseline(&[(ProbeId::ApiSurface, &[("NewThing", 5.0)])]);
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = vec![flow(&["NewThing"])];
        let out = attribute_from_baselines(a, &base, &head);
        let c = out.flows[0].cost.as_ref().unwrap();
        assert!(c.net > 0);
        assert_eq!(c.axes.continuation, 5);
    }

    #[test]
    fn baseline_summary_sums_per_probe_and_tokens() {
        let base = baseline_with_tokens(
            &[
                (ProbeId::ApiSurface, &[("A.x", 10.0), ("A.y", 4.0)]),
                (ProbeId::ExternalBoundaries, &[("A.x", 3.0)]),
                (ProbeId::TypeCallsites, &[("A.x", 6.0)]),
            ],
            &[("A.x", 500), ("A.y", 200)],
        );
        let head = baseline_with_tokens(
            &[
                (ProbeId::ApiSurface, &[("A.x", 9.0), ("A.y", 4.0)]),
                (ProbeId::ExternalBoundaries, &[("A.x", 3.0)]),
                (ProbeId::TypeCallsites, &[("A.x", 6.0)]),
            ],
            &[("A.x", 520), ("A.y", 200)],
        );
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = vec![flow(&["A.x"])];
        let out = attribute_from_baselines(a, &base, &head);
        let b = out.baseline.as_ref().unwrap();
        assert_eq!(b.axes_base.continuation, 14);
        assert_eq!(b.axes_base.operational, 3);
        assert_eq!(b.axes_base.runtime, 6);
        assert_eq!(b.tokens_base, 700);
        assert_eq!(b.tokens_head, 720);

        let c = out.flows[0].cost.as_ref().unwrap();
        // A.x tokens went 500 → 520 = +20 delta attributed to the flow.
        assert_eq!(c.tokens_delta, 20);
    }

    #[test]
    fn missing_entity_contributes_zero() {
        let base = baseline(&[(ProbeId::ApiSurface, &[("A.x", 5.0)])]);
        let head = baseline(&[(ProbeId::ApiSurface, &[("A.x", 5.0)])]);
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = vec![flow(&["Unknown.name"])];
        let out = attribute_from_baselines(a, &base, &head);
        let c = out.flows[0].cost.as_ref().unwrap();
        assert_eq!(c.net, 0);
    }

    // ---- Baseline pin plucking (RFC v0.3 §9) ---------------------------

    fn structural_artifact_with(flows: Vec<Flow>) -> Artifact {
        let base = baseline(&[(ProbeId::ApiSurface, &[])]);
        let head = baseline(&[(ProbeId::ApiSurface, &[])]);
        let mut a = Artifact::new(PrRef {
            repo: "r".into(),
            base_sha: "b".into(),
            head_sha: "h".into(),
        });
        a.flows = flows;
        attribute_from_baselines(a, &base, &head)
    }

    fn llm_flow(model: &str) -> Flow {
        let mut f = flow(&[]);
        f.source = FlowSource::Llm {
            model: model.into(),
            version: "v0.3.1".into(),
        };
        f
    }

    fn flow_with_proof(model: &str) -> Flow {
        use floe_core::evidence::Strength;
        use floe_core::intent::{Proof, ProofVerdict};
        let mut f = flow(&[]);
        f.proof = Some(Proof {
            verdict: ProofVerdict::NoIntent,
            strength: Strength::Low,
            reasoning: "t".into(),
            claims: Vec::new(),
            model: model.into(),
            prompt_version: "v0.1.0".into(),
        });
        f
    }

    #[test]
    fn baseline_pin_synthesis_model_plucked_from_llm_flow() {
        let out = structural_artifact_with(vec![llm_flow("glm-4.7")]);
        let pin = out.baseline.as_ref().unwrap();
        assert_eq!(pin.synthesis_model.as_deref(), Some("glm-4.7"));
    }

    #[test]
    fn baseline_pin_synthesis_model_none_when_all_structural() {
        let out = structural_artifact_with(vec![flow(&[])]);
        let pin = out.baseline.as_ref().unwrap();
        assert_eq!(pin.synthesis_model, None);
    }

    #[test]
    fn baseline_pin_proof_model_plucked_from_first_proof_flow() {
        // Mix: one flow with no proof, one with proof — pluck should
        // still find it regardless of order.
        let out = structural_artifact_with(vec![flow(&[]), flow_with_proof("glm-4.7")]);
        let pin = out.baseline.as_ref().unwrap();
        assert_eq!(pin.proof_model.as_deref(), Some("glm-4.7"));
    }

    #[test]
    fn baseline_pin_proof_model_none_when_proof_pass_skipped() {
        let out = structural_artifact_with(vec![flow(&[])]);
        let pin = out.baseline.as_ref().unwrap();
        assert_eq!(pin.proof_model, None);
    }
}
