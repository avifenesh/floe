import { useState } from "react";
import { cn } from "@/lib/cn";
import type { HunkClass } from "@/lib/artifact";
import type { DiffEntry, DiffRow, Segment, SkipBlock } from "@/lib/diff";
import { isSkip } from "@/lib/diff";
import type { HighlightedLines, Token } from "@/lib/highlight";
import type { InlineNote } from "@/types/artifact";
import { InlineNotes } from "@/components/InlineNotes";

export interface LineTouches {
  base: Map<number, Set<HunkClass>>;
  head: Map<number, Set<HunkClass>>;
}

interface Props {
  entries: DiffEntry[];
  baseTokens: HighlightedLines | null;
  headTokens: HighlightedLines | null;
  touches?: LineTouches;
  /** When present, each diff row gets a note affordance anchored to
   *  `(file, side, line)`. Existing notes render inline below the
   *  line; click "+ note" to compose a new one. */
  jobId?: string;
  file?: string;
  inlineNotes?: InlineNote[];
  onInlineNotesChange?: (next: InlineNote[]) => void;
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
export function DiffView({
  entries,
  baseTokens,
  headTokens,
  touches,
  jobId,
  file,
  inlineNotes,
  onInlineNotesChange,
}: Props) {
  const notesByLine = useLineNotesIndex(inlineNotes ?? [], file);
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
            jobId={jobId}
            file={file}
            allNotes={inlineNotes}
            notesForRow={collectRowNotes(entry, notesByLine)}
            onInlineNotesChange={onInlineNotesChange}
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
        {/* Note affordance is intentionally skipped inside skip-blocks —
            they're context fluff, not diff content worth annotating. */}
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
  jobId,
  file,
  allNotes,
  notesForRow,
  onInlineNotesChange,
}: {
  row: DiffRow;
  baseTokens: HighlightedLines | null;
  headTokens: HighlightedLines | null;
  touches?: LineTouches;
  jobId?: string;
  file?: string;
  allNotes?: InlineNote[];
  notesForRow?: InlineNote[];
  onInlineNotesChange?: (next: InlineNote[]) => void;
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
  const noteCapable = !!(jobId && file && onInlineNotesChange);
  // Prefer anchoring a new note on the head side (reviewer-facing
  // "current state"); fall back to base when this row is a pure
  // removal with no head line.
  const anchorSide: "base" | "head" = isRem && row.baseLine !== null ? "base" : "head";
  const anchorLine = anchorSide === "head" ? row.headLine : row.baseLine;

  return (
    <>
      <div
        className={cn(
          "flex items-stretch relative group",
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
      {noteCapable && anchorLine !== null && (notesForRow?.length || true) && (
        <LineNoteSlot
          jobId={jobId!}
          file={file!}
          side={anchorSide}
          line={anchorLine}
          notes={allNotes ?? []}
          rowNotes={notesForRow ?? []}
          onChange={onInlineNotesChange!}
        />
      )}
    </>
  );
}

/** Per-line note slot: shown below a diff row. Renders any existing
 *  notes anchored to `(file, side, line)` and a hover-revealed
 *  "+ note" affordance that expands the composer inline. */
function LineNoteSlot({
  jobId,
  file,
  side,
  line,
  notes,
  rowNotes,
  onChange,
}: {
  jobId: string;
  file: string;
  side: "base" | "head";
  line: number;
  notes: InlineNote[];
  rowNotes: InlineNote[];
  onChange: (next: InlineNote[]) => void;
}) {
  const [open, setOpen] = useState(false);
  // Reveal the composer only when either (a) a note already lives
  // here, or (b) the reviewer clicked the "+ note" hover affordance.
  if (rowNotes.length === 0 && !open) {
    return (
      <div className="flex items-center pl-[12rem] pr-3 py-0 leading-none h-0 group/slot relative">
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="opacity-0 group-hover/slot:opacity-100 translate-y-[-10px] text-[10px] font-mono text-muted-foreground hover:text-foreground bg-background border border-border/60 rounded px-1.5 py-0.5 transition-opacity"
          title={`Add note on ${side}:${line}`}
        >
          + note
        </button>
      </div>
    );
  }
  return (
    <div className="flex items-start border-y border-border/40 bg-muted/20 py-1">
      <div className="w-[12rem] shrink-0 text-[10px] font-mono text-muted-foreground pt-2 pl-3">
        {side}:{line}
      </div>
      <div className="flex-1 min-w-0 pr-3">
        <InlineNotes
          jobId={jobId}
          anchor={{ kind: "file-line", file, line_side: side, line }}
          notes={notes}
          onChange={onChange}
          label={`note on ${side}:${line}`}
        />
      </div>
    </div>
  );
}

/** Build a `${side}:${line}` → notes index for O(1) row lookup. */
function useLineNotesIndex(
  notes: InlineNote[],
  file: string | undefined,
): Map<string, InlineNote[]> {
  const out = new Map<string, InlineNote[]>();
  if (!file) return out;
  for (const n of notes) {
    if (n.anchor.kind !== "file-line") continue;
    if (n.anchor.file !== file) continue;
    const key = `${n.anchor.line_side}:${n.anchor.line}`;
    const list = out.get(key) ?? [];
    list.push(n);
    out.set(key, list);
  }
  return out;
}

function collectRowNotes(
  row: DiffRow,
  byKey: Map<string, InlineNote[]>,
): InlineNote[] {
  const out: InlineNote[] = [];
  if (row.baseLine !== null) {
    out.push(...(byKey.get(`base:${row.baseLine}`) ?? []));
  }
  if (row.headLine !== null) {
    out.push(...(byKey.get(`head:${row.headLine}`) ?? []));
  }
  return out;
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
  // The strip is the hover trigger for the sibling <ArchChip>. 3 px is a small
  // target visually, so we widen the *hit box* with a same-sized invisible
  // padding layer — visual weight stays 3 px, pointer can land on ~10 px.
  return (
    <div
      aria-hidden
      className={cn(
        "shrink-0 relative peer/strip",
        "w-[3px] hover:w-[5px] transition-[width] duration-100",
      )}
    >
      {/* invisible pointer padding — strip is 3 px visually, hit box ~11 px */}
      <div
        className="absolute inset-y-0 -inset-x-[4px]"
        style={{ pointerEvents: "auto" }}
      />
      <div className="w-full h-full bg-amber-500/70 hover:bg-amber-500 dark:bg-amber-400/60 dark:hover:bg-amber-400 relative transition-colors" />
    </div>
  );
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

/**
 * Small chip at the right edge of a flagged row. Compact initials by default
 * (e.g. `C·A` for Call + API); on hover *of the chip itself* it expands to
 * full labels. Hover scope is `group/chip` — hovering anywhere else on the
 * row or the strip does nothing.
 */
/**
 * Label that reveals on strip-hover only. Hidden by default (`opacity-0`);
 * becomes visible when the sibling `peer/strip` is hovered. No hover state
 * of its own — pointer-events-none so it never steals the hover focus
 * from the strip or lets the cursor enter it.
 */
function ArchChip({ kinds }: { kinds: Set<HunkClass> }) {
  const full = Array.from(kinds).map(kindLabel).join(" · ");
  return (
    <div
      aria-label={`Architectural: ${full}`}
      className={cn(
        "absolute right-2 top-1/2 -translate-y-1/2 pointer-events-none",
        "opacity-0 peer-hover/strip:opacity-100 transition-opacity duration-100",
        "text-[10px] font-mono font-medium tracking-wide rounded px-1.5 py-0.5",
        "bg-amber-100 text-amber-900 border border-amber-300",
        "dark:bg-amber-400/15 dark:text-amber-200 dark:border-amber-400/30",
      )}
    >
      {full}
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
