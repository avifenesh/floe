import { cn } from "@/lib/cn";
import type { DiffRow } from "@/lib/diff";

interface Props {
  rows: DiffRow[];
}

/**
 * Unified-style diff. One line per row, with line-number gutters on each side
 * and a tinted background on `add` / `remove`. The colored chrome around the
 * gutter is slightly stronger than the row background so the row itself reads
 * as code, not as a paint swatch. Intra-line word highlights land in pass 3.
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

  return (
    <div
      className={cn(
        "flex items-stretch",
        isAdd && "bg-emerald-500/10 dark:bg-emerald-400/10",
        isRem && "bg-rose-500/10 dark:bg-rose-400/10",
        isSkip && "bg-muted/40 text-muted-foreground italic",
      )}
    >
      <Gutter value={row.baseLine} mark={isRem ? "−" : null} tone={isRem ? "rem" : "equal"} />
      <Gutter value={row.headLine} mark={isAdd ? "+" : null} tone={isAdd ? "add" : "equal"} />
      <pre className="flex-1 px-3 py-[1px] whitespace-pre overflow-x-auto leading-5">
        {row.text || " "}
      </pre>
    </div>
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
        "w-14 shrink-0 flex items-center justify-end gap-1 px-2 py-[1px] select-none",
        "text-[11px] tabular-nums text-muted-foreground",
        tone === "add" && "bg-emerald-500/15 dark:bg-emerald-400/15 text-emerald-700 dark:text-emerald-300",
        tone === "rem" && "bg-rose-500/15 dark:bg-rose-400/15 text-rose-700 dark:text-rose-300",
      )}
    >
      <span className="w-2 text-center">{mark ?? ""}</span>
      <span>{value ?? ""}</span>
    </div>
  );
}
