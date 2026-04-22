import type { ArtifactBaseline } from "@/types/artifact";

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
