/**
 * Two-level navigation model, v0.2.
 *
 * Top row: one tab per detected flow, plus a PR tab for whole-PR views.
 * Second row: sub-tabs contextual to the top selection.
 *
 * The seven-view spine from v0.1 (`pr · flow · morph · delta · evidence ·
 * cost · source`) is gone — its views fold into either per-flow sub-tabs
 * or PR sub-tabs.
 */

/** What the top bar is pointing at. */
export type TopTab =
  | { kind: "flow"; flowId: string }
  | { kind: "pr" };

/** Per-flow sub-tabs. */
export type FlowSubTab = "overview" | "source" | "cost";

/** Whole-PR sub-tabs. `structure` is parked but reserved. */
export type PrSubTab = "flows-map" | "diff" | "cost" | "meta";

export const FLOW_SUB_TABS: { key: FlowSubTab; label: string }[] = [
  { key: "overview", label: "Overview" },
  { key: "source", label: "Source" },
  { key: "cost", label: "Cost" },
];

export const PR_SUB_TABS: { key: PrSubTab; label: string }[] = [
  { key: "flows-map", label: "Flows" },
  { key: "diff", label: "Diff" },
  { key: "cost", label: "Cost" },
  { key: "meta", label: "Meta" },
];
