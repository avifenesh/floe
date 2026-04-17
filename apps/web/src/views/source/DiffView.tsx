import { cn } from "@/lib/cn";
import type { DiffRow, Segment } from "@/lib/diff";

interface Props {
  rows: DiffRow[];
}

/**
 * Unified-style diff. Each row wraps at the column width so reviewers never
 * have to horizontal-scroll on reasonable widths. The row body tint is the
 * "soft" layer; when `segments` are present from the word-level pass, the
 * actual changed spans get a second, stronger tint on top. That means an
 * unchanged prefix/suffix of a modified line reads nearly as calm context,
 * and the eye is pulled only to the pieces that really differ.
 */
export function DiffView({ rows }: Props) {
  return (
    <div className="text-[12.5px] font-mono rounded border overflow-hidden">
      {rows.map((r, i) => (
        <Row key={i} row={r} />
      ))}
    </div>
  );
}

function Row({ row }: { row: DiffRow }) {
  const isAdd = row.kind === "add";
  const isRem = row.kind === "remove";
  const isSkip = row.kind === "equal" && row.baseLine === null && row.headLine === null;
  // A row with segments has some spans that are "equal" to the paired row.
  // On such rows we tone the whole-row background down, since the strong
  // span-level backgrounds will carry the real signal.
  const hasSegments = !!row.segments && row.segments.some((s) => s.kind === "equal");

  return (
    <div
      className={cn(
        "flex items-stretch",
        // Pure new/removed rows (no shared spans) get a *strong* full-row tint —
        // same intensity as the changed-span tint in paired rows, so "entirely
        // new code" and "the new piece of a modified line" read with the same
        // visual weight.
        isAdd && (hasSegments
          ? "bg-emerald-500/[0.06] dark:bg-emerald-400/[0.06]"
          : "bg-emerald-500/35 dark:bg-emerald-400/35"),
        isRem && (hasSegments
          ? "bg-rose-500/[0.06] dark:bg-rose-400/[0.06]"
          : "bg-rose-500/35 dark:bg-rose-400/35"),
        isSkip && "bg-muted/40 text-muted-foreground italic",
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
