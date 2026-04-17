import { cn } from "@/lib/cn";
import type { ChangedFile } from "@/lib/artifact";

interface Props {
  files: ChangedFile[];
  selected: string | null;
  onSelect: (path: string) => void;
}

/**
 * IDE-style tab strip for the Source view. One tab per changed file, shown
 * as `<status-dot> <path>`. The active tab carries a 2px foreground
 * underline (same pattern as the spine). Horizontally scrollable when the
 * file list outruns the width — no wrapping, since tab wrapping looks
 * worse than a subtle scroll.
 */
export function FileTabs({ files, selected, onSelect }: Props) {
  if (files.length === 0) return null;
  return (
    <div
      role="tablist"
      className="flex items-stretch border-b overflow-x-auto no-scrollbar"
    >
      {files.map((f, i) => (
        <Tab
          key={f.path}
          file={f}
          active={selected === f.path}
          onSelect={onSelect}
          withSeparator={i > 0}
        />
      ))}
    </div>
  );
}

function Tab({
  file,
  active,
  onSelect,
  withSeparator,
}: {
  file: ChangedFile;
  active: boolean;
  onSelect: (p: string) => void;
  withSeparator: boolean;
}) {
  return (
    <button
      role="tab"
      aria-selected={active}
      onClick={() => onSelect(file.path)}
      className={cn(
        "group relative flex items-center gap-2 px-3 h-9 shrink-0",
        "text-[12px] font-mono whitespace-nowrap transition-colors",
        withSeparator && "before:absolute before:left-0 before:top-2 before:bottom-2 before:w-px before:bg-border",
        active
          ? "text-foreground"
          : "text-muted-foreground hover:text-foreground hover:bg-muted/50",
      )}
    >
      <StatusDot status={file.status} />
      <span>{file.path}</span>
      {active && (
        <span
          aria-hidden
          className="absolute left-2 right-2 -bottom-[1px] h-[2px] bg-foreground rounded-full"
        />
      )}
    </button>
  );
}

function StatusDot({ status }: { status: ChangedFile["status"] }) {
  // Single glyph, color-differentiated — matches VS Code's gutter pattern.
  // One visual weight, eye scans by hue not by character shape.
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
