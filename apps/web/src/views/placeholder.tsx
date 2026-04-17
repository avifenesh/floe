import type { ViewKey } from "./types";

/**
 * Placeholder rendered for every view we haven't designed yet. Kept
 * deliberately boring — no layout decisions leaking into views that will
 * each get their own treatment.
 */
export function PlaceholderView({ view }: { view: ViewKey }) {
  return (
    <div className="flex items-center h-[40vh]">
      <div className="space-y-1">
        <div className="text-[13px] font-mono text-foreground">{view}</div>
        <div className="text-[13px] text-muted-foreground">not designed yet</div>
      </div>
    </div>
  );
}
