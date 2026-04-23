/** Subtle semantic color tokens for intent-fit / proof / cost signals.
 *
 *  Palette lives in one place so the whole product uses the same
 *  green-for-good / amber-for-partial / rose-for-missing vocabulary.
 *  Deliberately muted (border + soft bg + mid text) so the UI stays
 *  mostly neutral and color reads as signal, not decoration.
 *
 *  Tailwind class strings — tree-shaken at build; no runtime cost.
 */

import type { IntentFitVerdict, ProofVerdict } from "@/types/artifact";

type Tone = "good" | "partial" | "bad" | "neutral";

const TONE_PILL: Record<Tone, string> = {
  good:
    "border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200",
  partial:
    "border-amber-400/40 bg-amber-50 text-amber-800 dark:bg-amber-400/10 dark:text-amber-200",
  bad:
    "border-rose-400/40 bg-rose-50 text-rose-800 dark:bg-rose-400/10 dark:text-rose-200",
  neutral:
    "border-border/60 bg-background/60 text-muted-foreground",
};

const TONE_TEXT: Record<Tone, string> = {
  good: "text-emerald-700 dark:text-emerald-300",
  partial: "text-amber-700 dark:text-amber-300",
  bad: "text-rose-700 dark:text-rose-300",
  neutral: "text-muted-foreground",
};

function intentFitTone(v: IntentFitVerdict | undefined): Tone {
  switch (v) {
    case "delivers":
      return "good";
    case "partial":
      return "partial";
    case "unrelated":
      return "bad";
    case "no-intent":
    case undefined:
      return "neutral";
  }
}

function proofTone(v: ProofVerdict | undefined): Tone {
  switch (v) {
    case "strong":
      return "good";
    case "partial":
      return "partial";
    case "missing":
      return "bad";
    case "no-intent":
    case undefined:
      return "neutral";
  }
}

export function intentFitPillClass(v: IntentFitVerdict | undefined): string {
  return TONE_PILL[intentFitTone(v)];
}

export function proofPillClass(v: ProofVerdict | undefined): string {
  return TONE_PILL[proofTone(v)];
}

/** Signed-cost tone — MONOCHROME. The cost page reads as a single
 *  accent so numbers scan cleanly; direction comes from the sign and
 *  the arrow glyph from `signedCostArrow`, not from green-vs-red.
 *  (User directive: cost page "one color … easy to read but still
 *  appealing". Keeps red/emerald vocabulary for verdicts elsewhere.)
 */
export function signedCostTextClass(value: number | null | undefined): string {
  if (value === null || value === undefined || value === 0) return TONE_TEXT.neutral;
  return "text-foreground";
}

/** Unicode arrow that conveys direction without colour. Positive = up
 *  (harder navigation), negative = down (easier), zero = flat dash. */
export function signedCostArrow(value: number | null | undefined): string {
  if (value === null || value === undefined || value === 0) return "–";
  return value > 0 ? "↑" : "↓";
}
