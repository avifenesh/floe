export const VIEW_KEYS = [
  "pr",
  "flow",
  "morph",
  "delta",
  "evidence",
  "cost",
  "source",
] as const;

export type ViewKey = (typeof VIEW_KEYS)[number];

/** English display labels for the spine. `PR` stays a capital initialism;
 *  the rest are Title Case. */
export const VIEW_LABELS: Record<ViewKey, string> = {
  pr: "PR",
  flow: "Flow",
  morph: "Morph",
  delta: "Delta",
  evidence: "Evidence",
  cost: "Cost",
  source: "Source",
};
