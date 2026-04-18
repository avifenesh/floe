import type { Artifact, Flow } from "@/types/artifact";
import { cn } from "@/lib/cn";

interface Props {
  artifact: Artifact;
  onPick?: (flowId: string) => void;
}

/**
 * Flow overview on the PR page — the v0.2 primary landing. Each flow is a
 * card (name, source badge, rationale, counts). The banner above the list
 * fires when any flow is still `structural`: the host wanted LLM synthesis
 * but didn't get it (LLM off, unavailable, or rejected).
 *
 * No scope switching yet — this is the simplest cut. Click-to-scope lands
 * when the flow ribbon in the spine comes online.
 */
export function PrFlows({ artifact, onPick }: Props) {
  const flows = artifact.flows ?? [];
  if (flows.length === 0) {
    return null;
  }
  const anyStructural = flows.some((f) => (f.source as { kind: string }).kind === "structural");
  return (
    <section className="space-y-3">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Flows ({flows.length})
        </h2>
        {anyStructural && <StructuralBanner />}
      </div>
      <ol className="space-y-2">
        {flows.map((f) => (
          <li key={f.id}>
            <FlowCard flow={f} onPick={onPick} />
          </li>
        ))}
      </ol>
    </section>
  );
}

function FlowCard({ flow, onPick }: { flow: Flow; onPick?: (id: string) => void }) {
  const source = flow.source as { kind: string; model?: string; version?: string };
  const isStructural = source.kind === "structural";
  const clickable = !!onPick;
  return (
    <div
      onClick={clickable ? () => onPick!(flow.id) : undefined}
      role={clickable ? "button" : undefined}
      tabIndex={clickable ? 0 : undefined}
      onKeyDown={
        clickable
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                onPick!(flow.id);
              }
            }
          : undefined
      }
      className={cn(
        "rounded border px-3 py-2 space-y-1 transition-colors",
        isStructural
          ? "bg-muted/30 border-border/60"
          : "bg-amber-50 dark:bg-amber-400/5 border-amber-200 dark:border-amber-400/20",
        clickable && "cursor-pointer hover:bg-muted/60",
      )}
    >
      <div className="flex items-baseline gap-2">
        <span
          className={cn(
            "text-[13px] font-mono",
            isStructural ? "text-muted-foreground" : "text-foreground font-semibold",
          )}
        >
          {flow.name}
        </span>
        <SourceBadge source={source} />
        <span className="ml-auto text-[11px] font-mono text-muted-foreground tabular-nums">
          {flow.hunk_ids.length} hunk{flow.hunk_ids.length === 1 ? "" : "s"}
          {" · "}
          {flow.entities.length} entit{flow.entities.length === 1 ? "y" : "ies"}
        </span>
      </div>
      <p className="text-[12px] text-muted-foreground leading-relaxed">{flow.rationale}</p>
    </div>
  );
}

function SourceBadge({ source }: { source: { kind: string; model?: string } }) {
  if (source.kind === "structural") {
    return (
      <span className="text-[10px] font-mono tracking-wide px-1.5 py-0.5 rounded border border-border/60 text-muted-foreground">
        structural
      </span>
    );
  }
  return (
    <span className="text-[10px] font-mono tracking-wide px-1.5 py-0.5 rounded bg-emerald-100 text-emerald-900 border border-emerald-300 dark:bg-emerald-400/15 dark:text-emerald-200 dark:border-emerald-400/30">
      {source.model ? `llm: ${source.model}` : "llm"}
    </span>
  );
}

function StructuralBanner() {
  return (
    <span className="text-[10px] font-mono tracking-wide px-2 py-0.5 rounded bg-amber-100 text-amber-900 border border-amber-300 dark:bg-amber-400/15 dark:text-amber-200 dark:border-amber-400/30">
      Structural clustering — LLM synthesis not available
    </span>
  );
}
