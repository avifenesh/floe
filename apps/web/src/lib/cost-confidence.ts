/** Heuristic confidence for a per-flow cost number.
 *
 *  Schema v0 doesn't carry a real confidence field; the probe pass
 *  emits drivers and a net, no uncertainty estimate. Until scope 5's
 *  cost v2.3 (real coefficient fitting from `adr calibrate`) lands,
 *  we derive a coarse 0..1 confidence here so the UI can apply the
 *  RFC's "drivers first, net gated by confidence ≥ 0.70" rule.
 *
 *  Inputs that move confidence:
 *  - **Driver count** — one probe driver isn't enough signal to
 *    commit to a net; two or more is the floor for "confident".
 *  - **Sign agreement** — when drivers split on sign (one positive,
 *    one negative), the net is the difference of two noisy signals
 *    and shouldn't be trusted as a headline.
 *
 *  When this number < 0.70, callers grey the hero number and show a
 *  small "low confidence" hint instead of presenting the net as
 *  reliable.
 */

import type { Cost } from "@/types/artifact";

export const CONFIDENCE_THRESHOLD = 0.7;

export function costConfidence(cost: Cost | null | undefined): number {
  if (!cost) return 0;
  const drivers = cost.drivers ?? [];
  const nonZero = drivers.filter((d) => d.value !== 0);
  if (nonZero.length === 0) return 0.3;
  if (nonZero.length === 1) return 0.55;
  const positive = nonZero.filter((d) => d.value > 0).length;
  const negative = nonZero.filter((d) => d.value < 0).length;
  // All same sign: high confidence. Mixed signs: drop — the net is
  // a difference of competing signals.
  if (positive === 0 || negative === 0) return 0.85;
  return 0.55;
}

/** Aggregate confidence across multiple flow costs (PR-level view).
 *  Min, not mean — one low-confidence flow drags the aggregate down,
 *  which matches reviewer intuition better than averaging it away. */
export function aggregateCostConfidence(
  costs: (Cost | null | undefined)[],
): number {
  const present = costs.filter((c): c is Cost => c != null);
  if (present.length === 0) return 0;
  return Math.min(...present.map((c) => costConfidence(c)));
}
