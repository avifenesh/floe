import { useState } from "react";
import { cn } from "@/lib/cn";
import type { DiffEntry, DiffRow, Segment, SkipBlock } from "@/lib/diff";
import { isSkip } from "@/lib/diff";

interface Props {
  entries: DiffEntry[];
}

/**
 * Unified-style diff. Rows wrap at the column width; changed spans carry
 * strong tint while shared prefixes/suffixes of modified lines stay soft;
 * pure-new / pure-removed rows use the full strength. Collapsed context
 * appears as a clickable skip block that expands in place.
 */
export function DiffView({ entries }: Props) {
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
            />
          );
        }
        return <Row key={i} row={entry} />;
      })}
    </div>
  );
}

function Skip({
  block,
  open,
  onToggle,
}: {
  block: SkipBlock;
  open: boolean;
  onToggle: () => void;
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
          <Row key={idx} row={r} />
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

function Row({ row }: { row: DiffRow }) {
  const isAdd = row.kind === "add";
  const isRem = row.kind === "remove";
  const hasSegments = !!row.segments && row.segments.some((s) => s.kind === "equal");

  return (
    <div
      className={cn(
        "flex items-stretch",
        isAdd &&
          (hasSegments
            ? "bg-emerald-500/[0.06] dark:bg-emerald-400/[0.06]"
            : "bg-emerald-500/35 dark:bg-emerald-400/35"),
        isRem &&
          (hasSegments
            ? "bg-rose-500/[0.06] dark:bg-rose-400/[0.06]"
            : "bg-rose-500/35 dark:bg-rose-400/35"),
      )}
    >
      <Gutter value={row.baseLine} mark={isRem ? "−" : null} tone={isRem ? "rem" : "equal"} />
      <Gutter value={row.headLine} mark={isAdd ? "+" : null} tone={isAdd ? "add" : "equal"} />
      <pre className="flex-1 min-w-0 px-3 py-[1px] whitespace-pre-wrap break-words leading-5">
        {row.segments ? <Segments segments={row.segments} kind={row.kind} /> : row.text || " "}
      </pre>
    </div>
  );
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
                "bg-emerald-500/35 dark:bg-emerald-400/35 text-emerald-950 dark:text-emerald-50",
              kind === "remove" &&
                "bg-rose-500/35 dark:bg-rose-400/35 text-rose-950 dark:text-rose-50",
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
        "text-[11px] tabular-nums text-muted-foreground leading-5",
        tone === "add" && "bg-emerald-500/25 dark:bg-emerald-400/25 text-emerald-800 dark:text-emerald-200",
        tone === "rem" && "bg-rose-500/25 dark:bg-rose-400/25 text-rose-800 dark:text-rose-200",
      )}
    >
      <span className="w-2 text-center">{mark ?? ""}</span>
      <span>{value ?? ""}</span>
    </div>
  );
}
