import { diffLines, diffWordsWithSpace, type Change } from "diff";

export type DiffRowKind = "equal" | "add" | "remove";

/** One piece of a line, after the intra-line word diff has been overlaid. */
export interface Segment {
  kind: "equal" | "changed";
  text: string;
}

/**
 * One display row in a unified diff. `baseLine` / `headLine` are 1-based line
 * numbers on each side; either is null when the row exists only on one side.
 * `segments` is populated when the row has been paired with an opposite-side
 * row — then the renderer can tint only the differing spans strongly and
 * leave the shared prefix/suffix softly tinted.
 */
export interface DiffRow {
  kind: DiffRowKind;
  baseLine: number | null;
  headLine: number | null;
  text: string;
  segments?: Segment[];
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
 * Pair adjacent remove/add rows within each contiguous change block and
 * attach per-row `segments` computed from a word-level diff. A line pair gets
 * segments only when the two texts overlap meaningfully — if the word-level
 * diff says *everything* is changed, we treat the lines as fully new/removed
 * and skip the segment annotation (strong tint on the whole row is right).
 */
export function enrichWordLevel(rows: DiffRow[]): DiffRow[] {
  const out = rows.map((r) => ({ ...r }));
  let i = 0;
  while (i < out.length) {
    if (out[i].kind !== "remove") {
      i++;
      continue;
    }
    const removesStart = i;
    while (i < out.length && out[i].kind === "remove") i++;
    const addsStart = i;
    while (i < out.length && out[i].kind === "add") i++;
    const removes = out.slice(removesStart, addsStart);
    const adds = out.slice(addsStart, i);
    const pairs = Math.min(removes.length, adds.length);
    for (let k = 0; k < pairs; k++) {
      const rem = out[removesStart + k];
      const add = out[addsStart + k];
      const [remSegs, addSegs] = pairSegments(rem.text, add.text);
      if (remSegs && addSegs) {
        rem.segments = remSegs;
        add.segments = addSegs;
      }
    }
  }
  return out;
}

/** Returns segments for (base, head) if a useful word-level overlap exists,
 *  otherwise [null, null] so the row stays fully tinted. */
function pairSegments(a: string, b: string): [Segment[] | null, Segment[] | null] {
  const changes = diffWordsWithSpace(a, b);
  let equalLen = 0;
  let totalA = 0;
  let totalB = 0;
  for (const c of changes) {
    if (c.added) totalB += c.value.length;
    else if (c.removed) totalA += c.value.length;
    else {
      equalLen += c.value.length;
      totalA += c.value.length;
      totalB += c.value.length;
    }
  }
  // If barely any overlap, the lines are unrelated — don't annotate.
  const ratio = equalLen / Math.max(1, Math.max(totalA, totalB));
  if (ratio < 0.25) return [null, null];

  const remSegs: Segment[] = [];
  const addSegs: Segment[] = [];
  for (const c of changes) {
    if (c.added) {
      addSegs.push({ kind: "changed", text: c.value });
    } else if (c.removed) {
      remSegs.push({ kind: "changed", text: c.value });
    } else {
      remSegs.push({ kind: "equal", text: c.value });
      addSegs.push({ kind: "equal", text: c.value });
    }
  }
  return [remSegs, addSegs];
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
