import type { Artifact, IntentFitVerdict, ProofVerdict } from "@/types/artifact";

/** One-line verdict for "should this PR ship?", computed across the
 *  three signals the product already produces per flow:
 *
 *  - **intent-fit** — does this flow deliver something the intent claimed?
 *  - **proof** — is there evidence backing the claim(s)?
 *  - **cost** — did navigation cost move by a large signed amount?
 *
 *  The reviewer currently has to synthesise this across 3 tabs; this
 *  rolls it up and cites the weakest link so they can act.
 */
export type ShipState = "ready" | "caution" | "blocked" | "pending" | "no-intent";

export interface ShipReadiness {
  state: ShipState;
  /** One-sentence verdict shown as the headline. */
  headline: string;
  /** Optional pointer at the weakest flow/claim. */
  weakest?: {
    flowId: string;
    flowName: string;
    reason: string;
  };
  /** Counts so the UI can render pill counts next to the verdict. */
  counts: {
    flows: number;
    delivers: number;
    partial: number;
    unrelated: number;
    proof_strong: number;
    proof_partial: number;
    proof_missing: number;
  };
}

export function computeShipReadiness(artifact: Artifact): ShipReadiness {
  const flows = artifact.flows ?? [];
  const counts = {
    flows: flows.length,
    delivers: 0,
    partial: 0,
    unrelated: 0,
    proof_strong: 0,
    proof_partial: 0,
    proof_missing: 0,
  };
  let weakest: ShipReadiness["weakest"];

  // Proof pass hasn't run yet — can't verdict honestly.
  const proofStatus = artifact.proof_status ?? "not-run";
  if (proofStatus === "analyzing") {
    return {
      state: "pending",
      headline: "Intent-fit and proof still analysing — verdict will update when passes complete.",
      counts,
    };
  }
  if (!artifact.intent) {
    return {
      state: "no-intent",
      headline: "No intent supplied — proof pass has nothing to verify against.",
      counts,
    };
  }

  for (const f of flows) {
    const fit = f.intent_fit?.verdict as IntentFitVerdict | undefined;
    const pr = f.proof?.verdict as ProofVerdict | undefined;
    if (fit === "delivers") counts.delivers += 1;
    else if (fit === "partial") counts.partial += 1;
    else if (fit === "unrelated") counts.unrelated += 1;
    if (pr === "strong") counts.proof_strong += 1;
    else if (pr === "partial") counts.proof_partial += 1;
    else if (pr === "missing") counts.proof_missing += 1;

    // Weakest flow: worst-combination wins. Missing proof on a
    // delivers/partial flow matters more than unrelated (which is
    // just noise the PR happens to touch).
    const bad =
      (pr === "missing" && (fit === "delivers" || fit === "partial")) ||
      fit === "unrelated";
    if (bad && !weakest) {
      const reason =
        pr === "missing"
          ? `proof missing for "${f.name}"`
          : fit === "unrelated"
            ? `flow "${f.name}" is unrelated to any stated intent claim`
            : `flow "${f.name}" has weak evidence`;
      weakest = { flowId: f.id, flowName: f.name, reason };
    }
  }

  // Headline picks off counts: if anything is missing/unrelated → blocked
  // or caution; else ready.
  const anyMissing = counts.proof_missing > 0;
  const anyUnrelated = counts.unrelated > 0;
  const allStrong =
    flows.length > 0 &&
    counts.proof_strong === flows.length &&
    counts.unrelated === 0 &&
    counts.partial === 0;
  if (allStrong) {
    return {
      state: "ready",
      headline: "Every flow delivers a stated claim with strong evidence — safe to ship.",
      counts,
    };
  }
  if (anyMissing || anyUnrelated) {
    return {
      state: "blocked",
      headline: weakest
        ? `Block until resolved: ${weakest.reason}.`
        : "One or more flows have missing proof or unrelated intent — resolve before shipping.",
      weakest,
      counts,
    };
  }
  // Default: partial signals. Ship-at-your-own-risk.
  return {
    state: "caution",
    headline: weakest
      ? `Partial evidence on ${weakest.flowName} — worth a second look.`
      : "Partial evidence across flows — reviewable but not conclusive.",
    weakest,
    counts,
  };
}

export function shipStateClass(state: ShipState): string {
  switch (state) {
    case "ready":
      return "border-emerald-400/50 bg-emerald-50 text-emerald-900 dark:bg-emerald-400/10 dark:text-emerald-200";
    case "caution":
      return "border-amber-400/50 bg-amber-50 text-amber-900 dark:bg-amber-400/10 dark:text-amber-200";
    case "blocked":
      return "border-rose-400/50 bg-rose-50 text-rose-900 dark:bg-rose-400/10 dark:text-rose-200";
    case "pending":
    case "no-intent":
      return "border-border/60 bg-muted/30 text-muted-foreground";
  }
}

export function shipStateLabel(state: ShipState): string {
  switch (state) {
    case "ready":
      return "SHIP-READY";
    case "caution":
      return "CAUTION";
    case "blocked":
      return "BLOCKED";
    case "pending":
      return "ANALYSING";
    case "no-intent":
      return "NO INTENT";
  }
}
