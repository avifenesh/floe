import { cn } from "@/lib/cn";
import type { ChangedFile } from "@/lib/artifact";
import type { Flow } from "@/types/artifact";

interface Props {
  files: ChangedFile[];
  selected: string | null;
  onSelect: (path: string) => void;
  hunkCounts: Map<string, number>;
  flowsByFile: Map<string, Flow[]>;
}

/**
 * Left sidebar listing every changed file with its status dot, per-file hunk
 * count, and a dot-stack of the flow accent colors that touch it. Replaces
 * the previous horizontally-scrolling tab strip — with 39-file PRs like
 * glide-mq #181 the tab strip couldn't show everything at once.
 */
export function FileSidebar({ files, selected, onSelect, hunkCounts, flowsByFile }: Props) {
  if (files.length === 0) return null;
  return (
    <aside
      aria-label="Changed files"
      className="min-w-0 overflow-y-auto max-h-[calc(100vh-10rem)] text-muted-foreground/90"
    >
      <div className="px-1 py-1 text-[9px] font-mono tracking-wide uppercase text-muted-foreground/70">
        {files.length} file{files.length === 1 ? "" : "s"}
      </div>
      <ul role="listbox">
        {files.map((f) => {
          const active = selected === f.path;
          const short = shortPath(f.path);
          const dir = dirPath(f.path);
          const hunks = hunkCounts.get(f.path) ?? 0;
          const flows = flowsByFile.get(f.path) ?? [];
          return (
            <li key={f.path}>
              <button
                role="option"
                aria-selected={active}
                onClick={() => onSelect(f.path)}
                className={cn(
                  "w-full text-left px-1.5 py-0.5 text-[11px] font-mono flex items-center gap-1.5 min-w-0",
                  "transition-colors rounded-sm",
                  active
                    ? "bg-muted/50 text-foreground"
                    : "text-muted-foreground hover:text-foreground hover:bg-muted/30",
                )}
              >
                <StatusDot status={f.status} />
                <span className="flex-1 min-w-0 truncate" title={f.path}>
                  {dir && <span className="opacity-50">{dir}/</span>}
                  <span>{short}</span>
                </span>
                {flows.length > 0 && (
                  <span
                    className="shrink-0 text-[9px] tabular-nums opacity-60"
                    title={`${flows.length} flow${flows.length === 1 ? "" : "s"} touch this file`}
                  >
                    {flows.length}f
                  </span>
                )}
                {hunks > 0 && (
                  <span className="shrink-0 text-[9px] tabular-nums opacity-50">{hunks}</span>
                )}
              </button>
            </li>
          );
        })}
      </ul>
    </aside>
  );
}

function StatusDot({ status }: { status: ChangedFile["status"] }) {
  const tone =
    status === "added"
      ? "bg-emerald-500"
      : status === "removed"
        ? "bg-rose-500"
        : status === "modified"
          ? "bg-amber-500"
          : "bg-muted-foreground/40";
  return (
    <span
      aria-hidden
      className={cn("inline-block w-1.5 h-1.5 rounded-full shrink-0", tone)}
    />
  );
}

function shortPath(p: string): string {
  const i = p.lastIndexOf("/");
  return i === -1 ? p : p.slice(i + 1);
}

function dirPath(p: string): string {
  const i = p.lastIndexOf("/");
  return i === -1 ? "" : p.slice(0, i);
}
