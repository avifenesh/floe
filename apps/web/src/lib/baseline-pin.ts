import type { ArtifactBaseline } from "@/types/artifact";
import type { LlmConfigView } from "@/api";

/**
 * Returns true iff both baselines pin the same
 * `(probe_model, probe_set_version, synthesis_model, proof_model)`.
 * Two runs with different pins aren't apples-to-apples — the UI
 * should refuse to compare them or show a "re-baseline required"
 * banner per RFC v0.3 §9.
 *
 * Mirrors the Rust `ArtifactBaseline::pin_matches` exactly.
 * `null/undefined === null/undefined` counts as matching (neither
 * side ran that pass); `null !== "glm-4.7"` does not (one ran the
 * pass, the other didn't — not comparable).
 */
export function baselinePinMatches(
  a: ArtifactBaseline,
  b: ArtifactBaseline,
): boolean {
  return (
    a.probe_model === b.probe_model &&
    a.probe_set_version === b.probe_set_version &&
    (a.synthesis_model ?? null) === (b.synthesis_model ?? null) &&
    (a.proof_model ?? null) === (b.proof_model ?? null)
  );
}

/** One axis of drift between an artifact's pin and the live server
 *  config. `was` is what produced the displayed numbers; `now` is
 *  what a re-run would use. */
export interface DriftAxis {
  axis: "synthesis" | "probe" | "proof";
  was: string | null;
  now: string | null;
}

/** Detect per-axis drift between an artifact's baseline pin and the
 *  server's current LLM regime (from `GET /llm-config`).
 *
 *  Returns an empty array when everything matches. When non-empty,
 *  the caller should surface "re-baseline required" per RFC v0.3 §9
 *  — the displayed cost/proof numbers were produced by models the
 *  user is no longer running, so a re-run would produce different
 *  output.
 *
 *  `probe_set_version` isn't in `LlmConfigView` (it's probe-pipeline
 *  internal, not a user-facing env knob), so this helper only
 *  checks the three model axes. `baselinePinMatches` covers the
 *  full tuple for artifact-to-artifact compare.
 *
 *  Null-equality rule matches `baselinePinMatches`: `null === null`
 *  (neither ran that pass — no drift) but `null !== "glm-4.7"`
 *  (one ran the pass, the other won't — that's drift).
 */
export function detectBaselineDrift(
  baseline: ArtifactBaseline,
  cfg: LlmConfigView,
): DriftAxis[] {
  const out: DriftAxis[] = [];
  const check = (
    axis: DriftAxis["axis"],
    was: string | null | undefined,
    now: string | null | undefined,
  ): void => {
    const w = was ?? null;
    const n = now ?? null;
    if (w !== n) out.push({ axis, was: w, now: n });
  };
  check("synthesis", baseline.synthesis_model, cfg.synthesis_model);
  check("probe", baseline.probe_model, cfg.probe_model);
  check("proof", baseline.proof_model, cfg.proof_model);
  return out;
}
