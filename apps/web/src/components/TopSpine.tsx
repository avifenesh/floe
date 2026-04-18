import { cn } from "@/lib/cn";
import { useTheme } from "@/lib/theme";
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
  return (
    <header className="flex flex-col">
      <div className="h-10 flex items-center">
        <div className="w-full max-w-6xl mx-auto px-6 flex items-center gap-6">
          <div className="text-[12px] font-mono text-muted-foreground shrink-0 truncate max-w-[180px]">
            {prLabel ?? "No PR loaded"}
          </div>

          <nav className="flex items-center gap-4 min-w-0 flex-1 overflow-x-auto no-scrollbar">
            {flows.map((f) => (
              <TopTabButton
                key={f.id}
                active={top.kind === "flow" && top.flowId === f.id}
                onClick={() => onTop({ kind: "flow", flowId: f.id })}
                label={flowLabel(f)}
              />
            ))}
            {loaded && (
              <TopTabButton
                active={top.kind === "pr"}
                onClick={() => onTop({ kind: "pr" })}
                label="PR"
              />
            )}
          </nav>

          <div className="shrink-0 flex items-center gap-3">
            <button
              onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
              className="text-[11px] font-mono text-muted-foreground hover:text-foreground transition-colors"
              aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
            >
              {theme === "dark" ? "◐ Dark" : "○ Light"}
            </button>
            <div className="flex items-center gap-1.5">
              <span className="text-[11px] font-mono text-muted-foreground">Palette</span>
              <Kbd>/</Kbd>
            </div>
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
}: {
  active: boolean;
  onClick: () => void;
  label: string;
}) {
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
          className="absolute left-0 right-0 -bottom-[1px] h-[2px] bg-foreground rounded-full"
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

/** Shorten a flow's name for tab display — drop the `<structural: >` wrapper
 *  when present so the bucket stands on its own. */
function flowLabel(f: Flow): string {
  const m = f.name.match(/^<structural:\s*(.+?)>$/);
  return m ? m[1] : f.name;
}
