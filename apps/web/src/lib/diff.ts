import { diffLines, type Change } from "diff";

export type DiffRowKind = "equal" | "add" | "remove";

/**
 * One display row in a unified diff. `baseLine` / `headLine` are 1-based line
 * numbers on each side; either is null when the row exists only on one side.
 */
export interface DiffRow {
  kind: DiffRowKind;
  baseLine: number | null;
  headLine: number | null;
  text: string;
}

/**
 * Expand `diffLines` hunks into one DiffRow per line. `diff`'s `Change` has
 * `value` (joined lines), `added` / `removed` flags, and `count`.
 */
export function lineDiff(base: string, head: string): DiffRow[] {
  const changes: Change[] = diffLines(base, head);
  const rows: DiffRow[] = [];
  let baseLine = 1;
  let headLine = 1;
  for (const ch of changes) {
    // `value` ends with a trailing "\n" when the block was newline-terminated;
    // split and drop the resulting empty last element so we don't emit a phantom row.
    const lines = ch.value.split("\n");
    if (lines.length > 0 && lines[lines.length - 1] === "") lines.pop();
    for (const text of lines) {
      if (ch.added) {
        rows.push({ kind: "add", baseLine: null, headLine: headLine++, text });
      } else if (ch.removed) {
        rows.push({ kind: "remove", baseLine: baseLine++, headLine: null, text });
      } else {
        rows.push({ kind: "equal", baseLine: baseLine++, headLine: headLine++, text });
      }
    }
  }
  return rows;
}

/**
 * Keep changed rows plus `context` equal rows on each side. Returns the same
 * array shape so the renderer stays simple. Pass Infinity to disable clipping.
 */
export function clipContext(rows: DiffRow[], context = 3): DiffRow[] {
  if (!Number.isFinite(context)) return rows;
  const keep = new Array(rows.length).fill(false);
  rows.forEach((r, i) => {
    if (r.kind !== "equal") {
      for (let k = Math.max(0, i - context); k <= Math.min(rows.length - 1, i + context); k++) {
        keep[k] = true;
      }
    }
  });
  const out: DiffRow[] = [];
  let hidden = 0;
  rows.forEach((r, i) => {
    if (keep[i]) {
      if (hidden > 0) {
        out.push({
          kind: "equal",
          baseLine: null,
          headLine: null,
          text: `⋯ ${hidden} unchanged line${hidden === 1 ? "" : "s"}`,
        });
        hidden = 0;
      }
      out.push(r);
    } else {
      hidden += 1;
    }
  });
  return out;
}
