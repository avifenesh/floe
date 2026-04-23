import type { Artifact } from "@/types/artifact";
import { countFunctions, filesTouched } from "@/lib/artifact";

/**
 * PR-scope headline stats. Four tiles so the reviewer gets a scale
 * read (small / medium / large PR) at a glance. Was a ghostly
 * one-line prose strip; promoting to tiles makes it a proper stat
 * surface the page can lean on.
 */
export function PrStats({ artifact }: { artifact: Artifact }) {
  const files = filesTouched(artifact).length;
  const functions = countFunctions(artifact.head);
  const hunks = artifact.hunks.length;
  const flows = artifact.flows?.length ?? 0;
  return (
    <div className="grid grid-cols-2 md:grid-cols-4 gap-2">
      <Stat value={files} label={files === 1 ? "file" : "files"} />
      <Stat value={hunks} label={hunks === 1 ? "hunk" : "hunks"} />
      <Stat value={flows} label={flows === 1 ? "flow" : "flows"} />
      <Stat
        value={functions}
        label={functions === 1 ? "function" : "functions"}
      />
    </div>
  );
}

function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="rounded-md border border-border/60 bg-muted/60 shadow-sm px-3 py-2.5 flex items-baseline justify-between gap-3">
      <span className="text-[22px] font-mono font-semibold tabular-nums text-foreground leading-none">
        {value}
      </span>
      <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
        {label}
      </span>
    </div>
  );
}
