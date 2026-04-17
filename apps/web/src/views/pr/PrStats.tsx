import type { Artifact } from "@/types/artifact";
import { countFunctions, filesTouched } from "@/lib/artifact";

export function PrStats({ artifact }: { artifact: Artifact }) {
  const files = filesTouched(artifact).length;
  const functions = countFunctions(artifact.head);
  const hunks = artifact.hunks.length;
  return (
    <div className="flex items-baseline gap-6 text-[12px]">
      <Stat value={files} label={files === 1 ? "File" : "Files"} />
      <Stat value={functions} label={functions === 1 ? "Function" : "Functions"} />
      <Stat value={hunks} label={hunks === 1 ? "Hunk" : "Hunks"} />
    </div>
  );
}

function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="flex items-baseline gap-1.5">
      <span className="text-foreground font-mono font-semibold tabular-nums">{value}</span>
      <span className="text-muted-foreground">{label}</span>
    </div>
  );
}
