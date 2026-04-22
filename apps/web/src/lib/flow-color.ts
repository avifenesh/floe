import type { Flow } from "@/types/artifact";

/**
 * Deterministic per-flow accent color. A stable hash of the flow id picks one
 * of a fixed palette of visually-distant hues, then derives tints at alpha
 * values that read in both light and dark themes.
 *
 * The palette is hand-picked (not evenly spaced) so adjacent hues stay
 * distinguishable and no entry collides with the green/red diff tints.
 */
/**
 * Flow accent — intentionally a single neutral tone across every flow. An
 * earlier iteration hashed each flow id to a distinct hue; that produced a
 * rainbow of saturated accents that fought for attention and diluted the
 * product's "calm, editorial" feel. Flow identity is now carried entirely
 * by position + label.
 *
 * The function still accepts a Flow so call sites stay terse and future
 * per-flow variation (e.g. intensity based on weight) can slot in without
 * a ripple of changes.
 */
export interface FlowAccent {
  bg: string;
  bar: string;
  border: string;
}

const UNIFORM: FlowAccent = {
  bg: "hsl(0 0% 50% / 0.04)",
  bar: "hsl(0 0% 60% / 0.45)",
  border: "hsl(0 0% 60% / 0.3)",
};

export function flowAccent(_flowId: string): FlowAccent {
  return UNIFORM;
}

export function flowAccentByFlow(_f: Flow): FlowAccent {
  return UNIFORM;
}

/** Reviewer-facing flow label. Strips the `<structural: X>` wrapper from
 *  structural flow names and capitalizes lower-case bucket names (e.g.
 *  `top-level` → `Top-level`). Class-name buckets stay as-is. */
export function flowLabel(f: Flow): string {
  const m = f.name.match(/^<structural:\s*(.+?)>$/);
  const raw = m ? m[1] : f.name;
  if (raw.length > 0 && raw[0] >= "a" && raw[0] <= "z") {
    return raw[0].toUpperCase() + raw.slice(1);
  }
  return raw;
}

