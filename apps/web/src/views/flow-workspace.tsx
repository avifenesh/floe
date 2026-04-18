import type { Artifact, Flow } from "@/types/artifact";
import type { FlowSubTab } from "./types";
import { PrHunks } from "./pr/PrHunks";

interface Props {
  artifact: Artifact;
  jobId: string;
  flow: Flow;
  sub: FlowSubTab;
}

/**
 * Flow workspace. Each flow has its own set of sub-tabs; we render one at
 * a time based on the current sub selection.
 *
 * Overview is the first cut: the flow's header + its hunks. Source and
 * Cost are deliberate stubs — the real versions reuse the existing Source
 * view scoped to this flow's entities (lands next).
 */
export function FlowWorkspace({ artifact, jobId, flow, sub }: Props) {
  switch (sub) {
    case "overview":
      return <FlowOverview artifact={artifact} flow={flow} />;
    case "source":
      return <FlowSourceStub flow={flow} jobId={jobId} />;
    case "cost":
      return <FlowCostStub flow={flow} />;
  }
}

function FlowOverview({ artifact, flow }: { artifact: Artifact; flow: Flow }) {
  const flowHunks = artifact.hunks.filter((h) => flow.hunk_ids.includes(h.id));
  const scoped: Artifact = { ...artifact, hunks: flowHunks };
  return (
    <div className="space-y-5">
      <header className="space-y-1.5">
        <h1 className="text-[15px] font-semibold text-foreground">{flow.name}</h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          {flow.rationale}
        </p>
        <div className="flex items-baseline gap-4 pt-1 text-[11px] font-mono text-muted-foreground">
          <span>
            <span className="text-foreground font-semibold tabular-nums">
              {flow.hunk_ids.length}
            </span>{" "}
            hunk{flow.hunk_ids.length === 1 ? "" : "s"}
          </span>
          <span>
            <span className="text-foreground font-semibold tabular-nums">
              {flow.entities.length}
            </span>{" "}
            entit{flow.entities.length === 1 ? "y" : "ies"}
          </span>
        </div>
      </header>
      <section className="space-y-4">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Hunks in this flow
        </h2>
        <PrHunks artifact={scoped} />
      </section>
    </div>
  );
}

function FlowSourceStub({ flow, jobId: _jobId }: { flow: Flow; jobId: string }) {
  return (
    <div className="space-y-2">
      <h2 className="text-[13px] font-mono text-foreground">
        Source, scoped to {flow.name}
      </h2>
      <p className="text-[12px] text-muted-foreground max-w-3xl">
        Will render only the files whose hunks participate in this flow. Full
        file-tab Source rendering reuses the whole-PR Source component with a
        file filter — not wired yet.
      </p>
    </div>
  );
}

function FlowCostStub({ flow }: { flow: Flow }) {
  return (
    <div className="space-y-2">
      <h2 className="text-[13px] font-mono text-foreground">
        Cost, scoped to {flow.name}
      </h2>
      <p className="text-[12px] text-muted-foreground max-w-3xl">
        Per-flow cost (drivers + net) lands with the cost-model crate in
        scope 5. Stub for now.
      </p>
    </div>
  );
}
