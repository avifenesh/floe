import { useState } from "react";
import { cn } from "@/lib/cn";
import type { HunkClass } from "@/lib/artifact";
import type { DiffEntry, DiffRow, Segment, SkipBlock } from "@/lib/diff";
import { isSkip } from "@/lib/diff";
import type { HighlightedLines, Token } from "@/lib/highlight";

export interface LineTouches {
  base: Map<number, Set<HunkClass>>;
  head: Map<number, Set<HunkClass>>;
}

interface Props {
  entries: DiffEntry[];
  baseTokens: HighlightedLines | null;
  headTokens: HighlightedLines | null;
  touches?: LineTouches;
}

/**
 * Unified-style diff with syntax highlighting and a segment overlay.
 * Row body logic:
 *   - pure add / remove rows take full-strength tint;
 *   - paired rows (segments present) use a soft body so the strong
 *     per-span tint carries the real signal;
 *   - syntax colours come from shiki tokens, the segment background is
 *     layered *on top* so red/green strong tints win over the token
 *     colour for the changed spans.
 */
export function DiffView({ entries, baseTokens, headTokens, touches }: Props) {
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  return (
    <div className="text-[12.5px] font-mono rounded border overflow-hidden">
      {entries.map((entry, i) => {
        if (isSkip(entry)) {
          const isOpen = expanded.has(i);
          return (
            <Skip
              key={`skip-${i}`}
              block={entry}
              open={isOpen}
              onToggle={() =>
                setExpanded((s) => {
                  const next = new Set(s);
                  if (next.has(i)) next.delete(i);
                  else next.add(i);
                  return next;
                })
              }
              baseTokens={baseTokens}
              headTokens={headTokens}
              touches={touches}
            />
          );
        }
        return (
          <Row
            key={i}
            row={entry}
            baseTokens={baseTokens}
            headTokens={headTokens}
            touches={touches}
          />
        );
      })}
    </div>
  );
}

function Skip({
  block,
  open,
  onToggle,
  baseTokens,
  headTokens,
  touches,
}: {
  block: SkipBlock;
  open: boolean;
  onToggle: () => void;
  baseTokens: HighlightedLines | null;
  headTokens: HighlightedLines | null;
  touches?: LineTouches;
}) {
  if (open) {
    return (
      <>
        <button
          onClick={onToggle}
          className={cn(
            "flex items-stretch w-full text-left",
            "hover:bg-muted/50 transition-colors",
          )}
          aria-label={`Collapse ${block.hidden} unchanged lines`}
        >
          <div className="w-[6rem] shrink-0 flex items-center justify-center py-0.5 bg-muted/40 text-[10px] tracking-wide uppercase text-muted-foreground select-none">
            collapse
          </div>
          <div className="flex-1" />
        </button>
        {block.rows.map((r, idx) => (
          <Row
            key={idx}
            row={r}
            baseTokens={baseTokens}
            headTokens={headTokens}
            touches={touches}
          />
        ))}
      </>
    );
  }
  return (
    <button
      onClick={onToggle}
      className={cn(
        "flex items-stretch w-full text-left group",
        "bg-muted/30 hover:bg-muted/60 transition-colors",
      )}
      aria-label={`Expand ${block.hidden} unchanged line${block.hidden === 1 ? "" : "s"}`}
    >
      <div className="w-[6rem] shrink-0 flex items-center justify-center py-1 text-[10px] tracking-wide uppercase text-muted-foreground group-hover:text-foreground select-none">
        {block.hidden}
      </div>
      <div className="flex-1 px-3 py-1 text-[12px] text-muted-foreground group-hover:text-foreground">
        ⋯ click to show {block.hidden} unchanged line{block.hidden === 1 ? "" : "s"}
      </div>
    </button>
  );
}

function Row({
  row,
  baseTokens,
  headTokens,
  touches,
}: {
  row: DiffRow;
  baseTokens: HighlightedLines | null;
  headTokens: HighlightedLines | null;
  touches?: LineTouches;
}) {
  const isAdd = row.kind === "add";
  const isRem = row.kind === "remove";
  const hasSegments = !!row.segments && row.segments.some((s) => s.kind === "equal");

  // Pick the tokens for this row: added → head, removed → base, equal →
  // either (they match). Line numbers in the diff are 1-based.
  const tokens = lineTokens(row, baseTokens, headTokens);

  // Architectural flag: is this row's line part of an emitted hunk? Check
  // both sides — equal rows map to both; removed to base only; added to
  // head only.
  const archKinds = rowArchKinds(row, touches);

  const flagged = archKinds.size > 0;

  return (
    <div
      className={cn(
        "flex items-stretch relative group/row",
        isAdd &&
          (hasSegments
            ? "bg-emerald-50 dark:bg-emerald-400/[0.06]"
            : "bg-emerald-100 dark:bg-emerald-400/35"),
        isRem &&
          (hasSegments
            ? "bg-rose-50 dark:bg-rose-400/[0.06]"
            : "bg-rose-100 dark:bg-rose-400/35"),
      )}
    >
      <Gutter value={row.baseLine} mark={isRem ? "−" : null} tone={isRem ? "rem" : "equal"} />
      <Gutter value={row.headLine} mark={isAdd ? "+" : null} tone={isAdd ? "add" : "equal"} />
      <ArchStrip kinds={archKinds} />
      <pre className="flex-1 min-w-0 px-3 py-[1px] whitespace-pre-wrap break-words leading-5">
        <LineContent tokens={tokens} segments={row.segments ?? null} kind={row.kind} fallback={row.text} />
      </pre>
      {flagged && <ArchChip kinds={archKinds} />}
    </div>
  );
}

function rowArchKinds(row: DiffRow, touches?: LineTouches): Set<HunkClass> {
  const out = new Set<HunkClass>();
  if (!touches) return out;
  if (row.baseLine !== null) {
    const set = touches.base.get(row.baseLine);
    if (set) set.forEach((k) => out.add(k));
  }
  if (row.headLine !== null) {
    const set = touches.head.get(row.headLine);
    if (set) set.forEach((k) => out.add(k));
  }
  return out;
}

/**
 * 3-px vertical strip between the line-number gutters and the code. Present
 * only when the row belongs to an emitted hunk. Brightens + thickens on
 * row-hover (the companion `ArchChip` appears on the same hover), so the
 * strip alone doesn't need to be a precise click target.
 */
function ArchStrip({ kinds }: { kinds: Set<HunkClass> }) {
  if (kinds.size === 0) {
    return <div className="w-[3px] shrink-0" aria-hidden />;
  }
  return (
    <div
      aria-hidden
      className="w-[3px] shrink-0 bg-amber-500/70 dark:bg-amber-400/60"
    />
  );
}

/**
 * Always-visible kind label at the right edge of a flagged row. Kept quiet —
 * no background, no border, just a tiny amber monospace tag — so the row
 * still reads as code while the reviewer sees "this line belongs to Call · API"
 * at a glance.
 */
function ArchChip({ kinds }: { kinds: Set<HunkClass> }) {
  const label = Array.from(kinds).map(kindLabel).join(" · ");
  return (
    <div
      aria-hidden
      className="absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none"
    >
      <span className="text-[10px] font-mono font-medium tracking-wide text-amber-700 dark:text-amber-300">
        {label}
      </span>
    </div>
  );
}

function kindLabel(k: HunkClass): string {
  switch (k) {
    case "call":
      return "Call";
    case "state":
      return "State";
    case "api":
      return "API";
  }
}

function lineTokens(
  row: DiffRow,
  baseTokens: HighlightedLines | null,
  headTokens: HighlightedLines | null,
): Token[] | null {
  if (row.kind === "remove" && baseTokens && row.baseLine !== null) {
    return baseTokens[row.baseLine - 1] ?? null;
  }
  if (row.kind === "add" && headTokens && row.headLine !== null) {
    return headTokens[row.headLine - 1] ?? null;
  }
  if (row.kind === "equal") {
    if (headTokens && row.headLine !== null) return headTokens[row.headLine - 1] ?? null;
    if (baseTokens && row.baseLine !== null) return baseTokens[row.baseLine - 1] ?? null;
  }
  return null;
}

function LineContent({
  tokens,
  segments,
  kind,
  fallback,
}: {
  tokens: Token[] | null;
  segments: Segment[] | null;
  kind: "add" | "remove" | "equal";
  fallback: string;
}) {
  // No tokens (still loading or highlighter failed): fall back to segments
  // or plain text. The row background + span tint still carry the diff.
  if (!tokens) {
    if (!segments) return <>{fallback || " "}</>;
    return <Segments segments={segments} kind={kind} />;
  }
  const pieces = mergeTokensAndSegments(tokens, segments);
  return (
    <>
      {pieces.map((p, i) => (
        <span
          key={i}
          style={{ color: p.color }}
          className={cn(
            p.strong && kind === "add" &&
              "bg-emerald-300 dark:bg-emerald-400/40 rounded-[2px]",
            p.strong && kind === "remove" &&
              "bg-rose-300 dark:bg-rose-400/40 rounded-[2px]",
          )}
        >
          {p.content}
        </span>
      ))}
    </>
  );
}

interface MergedPiece {
  content: string;
  color?: string;
  strong: boolean;
}

/** Partition the line by both token boundaries and segment boundaries so
 *  each output piece carries one token colour + one segment kind. */
function mergeTokensAndSegments(
  tokens: Token[],
  segments: Segment[] | null,
): MergedPiece[] {
  if (!segments) {
    return tokens.map((t) => ({ content: t.content, color: t.color, strong: false }));
  }
  const out: MergedPiece[] = [];
  let ti = 0;
  let tOff = 0;
  let si = 0;
  let sOff = 0;
  while (ti < tokens.length && si < segments.length) {
    const t = tokens[ti];
    const s = segments[si];
    const tRem = t.content.length - tOff;
    const sRem = s.text.length - sOff;
    const take = Math.min(tRem, sRem);
    if (take > 0) {
      out.push({
        content: t.content.slice(tOff, tOff + take),
        color: t.color,
        strong: s.kind === "changed",
      });
    }
    tOff += take;
    sOff += take;
    if (tOff >= t.content.length) {
      ti += 1;
      tOff = 0;
    }
    if (sOff >= s.text.length) {
      si += 1;
      sOff = 0;
    }
  }
  return out;
}

function Segments({ segments, kind }: { segments: Segment[]; kind: "add" | "remove" | "equal" }) {
  return (
    <>
      {segments.map((s, i) => {
        if (s.kind === "equal") {
          return <span key={i}>{s.text}</span>;
        }
        return (
          <span
            key={i}
            className={cn(
              "rounded-[2px]",
              kind === "add" &&
                "bg-emerald-300 dark:bg-emerald-400/35 text-emerald-950 dark:text-emerald-50",
              kind === "remove" &&
                "bg-rose-300 dark:bg-rose-400/35 text-rose-950 dark:text-rose-50",
            )}
          >
            {s.text}
          </span>
        );
      })}
    </>
  );
}

function Gutter({
  value,
  mark,
  tone,
}: {
  value: number | null;
  mark: string | null;
  tone: "add" | "rem" | "equal";
}) {
  return (
    <div
      className={cn(
        "w-12 shrink-0 flex items-start justify-end gap-1 px-2 py-[2px] select-none",
        "text-[11px] tabular-nums leading-5 text-muted-foreground",
        tone === "add" &&
          "bg-emerald-200 text-emerald-900 dark:bg-emerald-400/25 dark:text-emerald-200",
        tone === "rem" &&
          "bg-rose-200 text-rose-900 dark:bg-rose-400/25 dark:text-rose-200",
      )}
    >
      <span className="w-2 text-center">{mark ?? ""}</span>
      <span>{value ?? ""}</span>
    </div>
  );
}
