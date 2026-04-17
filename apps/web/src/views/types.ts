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
