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

/** Per-flow sub-tabs — the full set we'll try visually; a few are stubbed
 *  while we decide the order and which ones earn space. */
export type FlowSubTab =
  | "overview"
  | "flow"
  | "morph"
  | "delta"
  | "evidence"
  | "source"
  | "cost";

/** Whole-PR sub-tabs. */
export type PrSubTab =
  | "flows-map"
  | "diff"
  | "structure"
  | "cost"
  | "meta";

export const FLOW_SUB_TABS: { key: FlowSubTab; label: string }[] = [
  { key: "overview", label: "Overview" },
  { key: "flow", label: "Flow" },
  { key: "morph", label: "Morph" },
  { key: "delta", label: "Delta" },
  { key: "evidence", label: "Evidence" },
  { key: "source", label: "Source" },
  { key: "cost", label: "Cost" },
];

export const PR_SUB_TABS: { key: PrSubTab; label: string }[] = [
  { key: "flows-map", label: "Flows" },
  { key: "diff", label: "Diff" },
  { key: "structure", label: "Structure" },
  { key: "cost", label: "Cost" },
  { key: "meta", label: "Meta" },
];
