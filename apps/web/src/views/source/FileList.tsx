import { cn } from "@/lib/cn";
import type { ChangedFile } from "@/lib/artifact";

interface Props {
  files: ChangedFile[];
  selected: string | null;
  onSelect: (path: string) => void;
}

export function FileList({ files, selected, onSelect }: Props) {
  if (files.length === 0) {
    return (
      <div className="text-[12px] text-muted-foreground">No files in the artifact.</div>
    );
  }
  return (
    <ul className="space-y-0.5">
      {files.map((f) => (
        <li key={f.path}>
          <button
            onClick={() => onSelect(f.path)}
            className={cn(
              "w-full text-left text-[12px] font-mono flex items-center gap-2 py-1 px-2 rounded transition-colors",
              selected === f.path
                ? "bg-muted text-foreground"
                : "text-muted-foreground hover:text-foreground hover:bg-muted/60",
            )}
          >
            <StatusMark status={f.status} />
            <span className="truncate">{f.path}</span>
          </button>
        </li>
      ))}
    </ul>
  );
}

function StatusMark({ status }: { status: ChangedFile["status"] }) {
  const mark =
    status === "added" ? "+" : status === "removed" ? "−" : status === "modified" ? "~" : " ";
  return (
    <span
      className="w-3 inline-block text-muted-foreground tabular-nums"
      aria-label={status}
    >
      {mark}
    </span>
  );
}
