import type { Artifact, Flow } from "@/types/artifact";
import { cn } from "@/lib/cn";
import { filesTouched, flowHunks, hunkTypeCounts } from "@/lib/artifact";
import { flowLabel } from "@/lib/flow-color";

interface Props {
  artifact: Artifact;
  onPick?: (flowId: string) => void;
}

/**
 * Flow overview on the PR page — the v0.2 primary landing. Each flow is a
 * card (name, source badge, rationale, counts). Flows are sorted by weight
 * (hunks + entities) so the heaviest lands first.
 */
export function PrFlows({ artifact, onPick }: Props) {
  const flows = artifact.flows ?? [];
  if (flows.length === 0) {
    return null;
  }
  const ranked = [...flows].sort((a, b) => weight(b) - weight(a));
  const maxWeight = Math.max(...ranked.map(weight), 1);

  return (
    <section className="space-y-4">
      <ScaleStrip artifact={artifact} />
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Flows ({flows.length}) · ranked by weight
        </h2>
      </div>
      <ol className="space-y-2">
        {ranked.map((f) => (
          <li key={f.id}>
            <FlowCard
              artifact={artifact}
              flow={f}
              onPick={onPick}
              maxWeight={maxWeight}
            />
          </li>
        ))}
      </ol>
    </section>
  );
}

function weight(f: Flow): number {
  return f.hunk_ids.length * 2 + f.entities.length;
}

/* -------------------------------------------------------------------------- */
/* PR scale strip                                                             */
/* -------------------------------------------------------------------------- */

function ScaleStrip({ artifact }: { artifact: Artifact }) {
  const files = filesTouched(artifact).length;
  const flows = artifact.flows?.length ?? 0;
  const counts = hunkTypeCounts(artifact.hunks);
  const entities = new Set<string>();
  for (const f of artifact.flows ?? []) for (const e of f.entities) entities.add(e);
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-4 py-3 flex flex-wrap items-center gap-x-6 gap-y-2">
      <Stat value={files} label={files === 1 ? "file" : "files"} />
      <Stat value={counts.total} label={counts.total === 1 ? "hunk" : "hunks"} />
      <Stat value={flows} label={flows === 1 ? "flow" : "flows"} />
      <Stat value={entities.size} label={entities.size === 1 ? "entity" : "entities"} />
      <div className="ml-auto shrink-0">
        <TypeRatioBar counts={counts} />
      </div>
    </div>
  );
}

function Stat({ value, label }: { value: number; label: string }) {
  return (
    <div className="flex items-baseline gap-1.5 text-[12px] font-mono">
      <span className="text-foreground font-semibold tabular-nums">{value}</span>
      <span className="text-muted-foreground">{label}</span>
    </div>
  );
}

function TypeRatioBar({
  counts,
}: {
  counts: { call: number; state: number; api: number; total: number };
}) {
  return (
    <div className="flex items-baseline gap-3 text-[11px] font-mono text-muted-foreground">
      <HunkTypeCount label="call" n={counts.call} />
      <HunkTypeCount label="state" n={counts.state} />
      <HunkTypeCount label="api" n={counts.api} />
    </div>
  );
}

function HunkTypeCount({ label, n }: { label: string; n: number }) {
  return (
    <span className="inline-flex items-baseline gap-1">
      <span className="text-foreground font-semibold tabular-nums">{n}</span>
      <span>{label}</span>
    </span>
  );
}

/* -------------------------------------------------------------------------- */
/* Flow card                                                                  */
/* -------------------------------------------------------------------------- */

function FlowCard({
  artifact,
  flow,
  onPick,
  maxWeight,
}: {
  artifact: Artifact;
  flow: Flow;
  onPick?: (id: string) => void;
  maxWeight: number;
}) {
  const source = flow.source as { kind: string; model?: string; version?: string };
  const isStructural = source.kind === "structural";
  const clickable = !!onPick;
  const scopedHunks = flowHunks(artifact, flow.hunk_ids);
  const counts = hunkTypeCounts(scopedHunks);
  const label = flowLabel(flow);
  const w = flow.hunk_ids.length * 2 + flow.entities.length;
  const widthPct = Math.max(4, Math.round((w / maxWeight) * 100));
  const topEntities = flow.entities.slice(0, 3);
  const extraEntities = Math.max(0, flow.entities.length - topEntities.length);

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
        "rounded-r border border-l-[3px] border-border/60 border-l-muted-foreground/30 bg-muted/20 px-3 py-2.5 space-y-2 transition-colors",
        clickable && "cursor-pointer hover:bg-muted/40",
      )}
    >
      <div className="flex items-baseline gap-2">
        <span className="text-[13px] font-mono font-semibold text-foreground">
          {label}
        </span>
        {isStructural ? (
          <span className="text-[10px] font-mono tracking-wide px-1.5 py-0.5 rounded border border-border/60 text-muted-foreground">
            structural
          </span>
        ) : (
          <span className="text-[10px] font-mono tracking-wide px-1.5 py-0.5 rounded bg-emerald-100 text-emerald-900 border border-emerald-300 dark:bg-emerald-400/15 dark:text-emerald-200 dark:border-emerald-400/30">
            {source.model ? `llm: ${source.model}` : "llm"}
          </span>
        )}
        <span className="ml-auto text-[11px] font-mono text-muted-foreground tabular-nums">
          {flow.hunk_ids.length}h · {flow.entities.length}e
        </span>
      </div>

      {/* Weight bar — visual anchor for "how heavy is this flow?" */}
      <div className="h-[3px] rounded-full bg-muted overflow-hidden">
        <div
          className="h-full rounded-full bg-muted-foreground/40"
          style={{ width: `${widthPct}%` }}
        />
      </div>

      {/* Top entities + type ratio pills — what's actually in the flow */}
      <div className="flex flex-wrap items-center gap-1.5">
        {topEntities.map((e) => (
          <span
            key={e}
            className="text-[11px] font-mono px-1.5 py-0.5 rounded bg-background/60 border border-border/50 text-foreground/80"
          >
            {e}
          </span>
        ))}
        {extraEntities > 0 && (
          <span className="text-[11px] font-mono text-muted-foreground">
            +{extraEntities} more
          </span>
        )}
        <span className="ml-auto flex items-center gap-2 text-[10px] font-mono text-muted-foreground">
          {counts.call > 0 && <TypePill kind="call" n={counts.call} />}
          {counts.state > 0 && <TypePill kind="state" n={counts.state} />}
          {counts.api > 0 && <TypePill kind="api" n={counts.api} />}
        </span>
      </div>
    </div>
  );
}

function TypePill({ kind, n }: { kind: "call" | "state" | "api"; n: number }) {
  return (
    <span className="text-muted-foreground">
      {kind} {n}
    </span>
  );
}

