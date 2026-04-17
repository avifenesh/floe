import { cn } from "@/lib/cn";
import type { ChangedFile } from "@/lib/artifact";

interface Props {
  files: ChangedFile[];
  selected: string | null;
  onSelect: (path: string) => void;
}

/**
 * Minimal file rail. Collapsed by default to a narrow strip of status marks;
 * expands into the reading area on hover so the diff never reflows. Each
 * file shows a status dot (+ added · − removed · ~ modified · · unchanged)
 * and, on hover, its full path.
 *
 * We use `group` + `group-hover` plus absolute positioning for the name
 * column so hovering the rail reveals names in an overlay without pushing
 * the diff around.
 */
export function FileList({ files, selected, onSelect }: Props) {
  if (files.length === 0) {
    return (
      <div className="text-[12px] text-muted-foreground">No files in the artifact.</div>
    );
  }
  return (
    <div className="group/rail relative w-8">
      <ul className="relative z-10 flex flex-col gap-1 py-0.5">
        {files.map((f) => (
          <li key={f.path} className="relative">
            <button
              onClick={() => onSelect(f.path)}
              title={f.path}
              className={cn(
                "flex items-center gap-2 w-8 group-hover/rail:w-56 h-6 px-2 rounded-sm",
                "text-[12px] font-mono whitespace-nowrap transition-[width,background] duration-150",
                "bg-transparent",
                selected === f.path
                  ? "text-foreground"
                  : "text-muted-foreground hover:text-foreground",
                selected === f.path &&
                  "bg-muted/80 group-hover/rail:bg-muted",
              )}
              aria-label={`${f.path} (${f.status})`}
            >
              <StatusDot status={f.status} />
              <span className="opacity-0 group-hover/rail:opacity-100 transition-opacity duration-150 truncate">
                {f.path}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </div>
  );
}

function StatusDot({ status }: { status: ChangedFile["status"] }) {
  const tone =
    status === "added"
      ? "text-emerald-500"
      : status === "removed"
        ? "text-rose-500"
        : status === "modified"
          ? "text-amber-500"
          : "text-muted-foreground/50";
  const mark =
    status === "added" ? "+" : status === "removed" ? "−" : status === "modified" ? "●" : "·";
  return (
    <span className={cn("w-3 inline-block text-center text-[13px] leading-none", tone)} aria-hidden>
      {mark}
    </span>
  );
}
