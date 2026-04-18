import type { Artifact } from "@/types/artifact";
import type { PrSubTab, TopTab } from "./types";
import { PrFlows } from "./pr/PrFlows";
import { PrHeader } from "./pr/PrHeader";
import { PrStats } from "./pr/PrStats";
import { PrHunks } from "./pr/PrHunks";
import { SourceView } from "./source";

interface Props {
  artifact: Artifact;
  jobId: string;
  sub: PrSubTab;
  onTop: (t: TopTab) => void;
}

/**
 * Whole-PR workspace. Sub-tabs:
 *   flows-map — overview of detected flows; click a flow card to open its
 *     top-tab workspace.
 *   diff — the full textual diff, unscoped (the existing Source view).
 *   cost — aggregate PR cost (stub).
 *   meta — identity header + stats + raw hunk list.
 *
 * "Structure" — code-surface view (classes, modules, exported symbols) —
 * is reserved as a future sub-tab and not in the first iteration.
 */
export function PrWorkspace({ artifact, jobId, sub, onTop }: Props) {
  switch (sub) {
    case "flows-map":
      return <FlowsMap artifact={artifact} onTop={onTop} />;
    case "diff":
      return <SourceView artifact={artifact} jobId={jobId} />;
    case "structure":
      return <StructureStub />;
    case "cost":
      return <CostStub />;
    case "meta":
      return <Meta artifact={artifact} />;
  }
}

function StructureStub() {
  return (
    <div className="space-y-2">
      <h2 className="text-[13px] font-mono text-foreground">Structure</h2>
      <p className="text-[12px] text-muted-foreground max-w-3xl">
        Code-surface view independent of the flow cut: classes, interfaces,
        modules, and their exported symbols. Stub for now — lands after the
        analyzer grows a class / interface extraction pass.
      </p>
    </div>
  );
}

function FlowsMap({
  artifact,
  onTop,
}: {
  artifact: Artifact;
  onTop: (t: TopTab) => void;
}) {
  return (
    <div className="space-y-5">
      <PrFlows
        artifact={artifact}
        onPick={(flowId) => onTop({ kind: "flow", flowId })}
      />
    </div>
  );
}

function Meta({ artifact }: { artifact: Artifact }) {
  return (
    <div className="space-y-6">
      <PrHeader artifact={artifact} />
      <PrStats artifact={artifact} />
      <section className="space-y-4">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Architectural delta
        </h2>
        <PrHunks artifact={artifact} />
      </section>
    </div>
  );
}

function CostStub() {
  return (
    <div className="space-y-2">
      <h2 className="text-[13px] font-mono text-foreground">PR cost</h2>
      <p className="text-[12px] text-muted-foreground max-w-3xl">
        Aggregate cost across all flows — drivers + net — lands with the
        cost-model crate in scope 5. Stub for now.
      </p>
    </div>
  );
}
