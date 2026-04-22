import { cn } from "@/lib/cn";
import { useTheme } from "@/lib/theme";
import { flowLabel } from "@/lib/flow-color";
import type { Flow } from "@/types/artifact";
import {
  FLOW_SUB_TABS,
  PR_SUB_TABS,
  type FlowSubTab,
  type PrSubTab,
  type TopTab,
} from "@/views/types";
import { Kbd } from "./Kbd";

interface Props {
  prLabel: string | null;
  flows: Flow[];
  top: TopTab;
  onTop: (t: TopTab) => void;
  flowSub: FlowSubTab;
  onFlowSub: (s: FlowSubTab) => void;
  prSub: PrSubTab;
  onPrSub: (s: PrSubTab) => void;
}

/**
 * Two-row spine.
 *   Row 1: PR identity · per-flow tabs + PR · theme + palette.
 *   Row 2: sub-tabs (flow sub-tabs when a flow is selected; PR sub-tabs
 *     when PR is selected).
 * The second row only renders when a PR is loaded.
 */
export function TopSpine({
  prLabel,
  flows,
  top,
  onTop,
  flowSub,
  onFlowSub,
  prSub,
  onPrSub,
}: Props) {
  const [theme, setTheme] = useTheme();
  const loaded = prLabel !== null;

  // Left-anchored nav: PR first, then top-level (if present), then class flows.
  const topLevelIdx = flows.findIndex((f) => flowLabel(f).toLowerCase() === "top-level");
  const topLevel = topLevelIdx >= 0 ? flows[topLevelIdx] : null;
  const otherFlows = topLevelIdx >= 0 ? flows.filter((_, i) => i !== topLevelIdx) : flows;

  return (
    <header className="flex flex-col">
      <div className="h-10 flex items-center">
        <div className="w-full max-w-6xl mx-auto px-6 flex items-center gap-4">
          {/* Identity sits on the LEFT now, truncates aggressively. The
              old right-side anchor competed with the flow tab row for
              horizontal space and clipped the 3rd+ tabs behind itself
              on typical viewports. */}
          <div
            className="text-[12px] font-mono text-muted-foreground shrink-0 truncate max-w-[160px]"
            title={prLabel ?? "No PR loaded"}
          >
            {prLabel ?? "No PR loaded"}
          </div>
          {loaded && (
            <span
              aria-hidden
              className="h-4 w-px bg-border shrink-0"
            />
          )}
          <nav className="flex items-center gap-4 min-w-0 flex-1 overflow-x-auto no-scrollbar">
            {loaded && (
              <TopTabButton
                active={top.kind === "pr"}
                onClick={() => onTop({ kind: "pr" })}
                label="PR"
              />
            )}
            {loaded && flows.length > 0 && (
              <>
                <span
                  aria-hidden
                  className="h-4 w-px bg-border shrink-0"
                />
                <span className="text-[10px] font-mono tracking-[0.12em] uppercase text-muted-foreground shrink-0">
                  Flows:
                </span>
              </>
            )}
            {topLevel && (
              <TopTabButton
                key={topLevel.id}
                active={top.kind === "flow" && top.flowId === topLevel.id}
                onClick={() => onTop({ kind: "flow", flowId: topLevel.id })}
                label={flowLabel(topLevel)}
                framed
              />
            )}
            {otherFlows.map((f) => (
              <TopTabButton
                key={f.id}
                active={top.kind === "flow" && top.flowId === f.id}
                onClick={() => onTop({ kind: "flow", flowId: f.id })}
                label={flowLabel(f)}
                framed
              />
            ))}
          </nav>

          <div className="shrink-0 flex items-center gap-2">
            <button
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
              className="inline-flex items-center gap-1.5 text-[11px] font-mono text-muted-foreground hover:text-foreground rounded-md border border-border/60 bg-background/60 hover:bg-muted/40 px-2 py-1 transition-colors"
              aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
              title={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
            >
              <span aria-hidden>{theme === "dark" ? "◐" : "○"}</span>
              <span>{theme === "dark" ? "Dark" : "Light"}</span>
            </button>
            <button
              onClick={() => window.dispatchEvent(new CustomEvent("adr:open-palette"))}
              className="inline-flex items-center gap-1.5 text-[11px] font-mono text-muted-foreground hover:text-foreground rounded-md border border-border/60 bg-background/60 hover:bg-muted/40 px-2 py-1 transition-colors"
              aria-label="Open command palette"
              title="Open command palette"
            >
              <span>Palette</span>
              <Kbd>/</Kbd>
            </button>
          </div>
        </div>
      </div>
      {loaded && (
        <div className="h-8 flex items-center">
          <div className="w-full max-w-6xl mx-auto px-6 flex items-center gap-4">
            {top.kind === "flow"
              ? FLOW_SUB_TABS.map((s) => (
                  <SubTabButton
                    key={s.key}
                    active={flowSub === s.key}
                    onClick={() => onFlowSub(s.key)}
                    label={s.label}
                  />
                ))
              : PR_SUB_TABS.map((s) => (
                  <SubTabButton
                    key={s.key}
                    active={prSub === s.key}
                    onClick={() => onPrSub(s.key)}
                    label={s.label}
                  />
                ))}
          </div>
        </div>
      )}
    </header>
  );
}

function TopTabButton({
  active,
  onClick,
  label,
  framed,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
  framed?: boolean;
}) {
  if (framed) {
    return (
      <button
        onClick={onClick}
        className={cn(
          "text-[12px] tracking-tight transition-colors shrink-0 whitespace-nowrap px-2.5 py-1 rounded border",
          active
            ? "text-foreground font-semibold border-foreground/60 bg-muted/60"
            : "text-muted-foreground font-medium border-border/50 hover:text-foreground hover:border-border",
        )}
      >
        {label}
      </button>
    );
  }
  return (
    <button
      onClick={onClick}
      className={cn(
        "text-[13px] tracking-tight transition-colors relative py-1 shrink-0 whitespace-nowrap",
        active
          ? "text-foreground font-semibold"
          : "text-muted-foreground font-medium hover:text-foreground",
      )}
    >
      {label}
      {active && (
        <span
          aria-hidden
          className="absolute left-0 right-0 -bottom-[1px] h-[2px] rounded-full bg-foreground"
        />
      )}
    </button>
  );
}

function SubTabButton({
  active,
  onClick,
  label,
}: {
  active: boolean;
  onClick: () => void;
  label: string;
}) {
  return (
    <button
      onClick={onClick}
      className={cn(
        "text-[11px] tracking-wide transition-colors relative py-0.5",
        active
          ? "text-foreground font-medium"
          : "text-muted-foreground hover:text-foreground",
      )}
    >
      {label}
      {active && (
        <span
          aria-hidden
          className="absolute left-0 right-0 -bottom-[1px] h-[1.5px] bg-foreground/70 rounded-full"
        />
      )}
    </button>
  );
}

