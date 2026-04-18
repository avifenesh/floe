import { useMemo, useState } from "react";
import { TopSpine } from "@/components/TopSpine";
import { LoadForm } from "@/views/load-form";
import { FlowWorkspace } from "@/views/flow-workspace";
import { PrWorkspace } from "@/views/pr-workspace";
import type { FlowSubTab, PrSubTab, TopTab } from "@/views/types";
import type { Artifact } from "@/types/artifact";
import { deriveSlug, isPathSha, shortSha } from "@/lib/artifact";

export interface LoadedJob {
  jobId: string;
  artifact: Artifact;
}

export default function App() {
  const [job, setJob] = useState<LoadedJob | null>(null);
  const [top, setTop] = useState<TopTab>({ kind: "pr" });
  const [flowSub, setFlowSub] = useState<FlowSubTab>("overview");
  const [prSub, setPrSub] = useState<PrSubTab>("flows-map");

  const flows = job?.artifact.flows ?? [];
  const selectedFlow = useMemo(() => {
    if (top.kind !== "flow") return null;
    return flows.find((f) => f.id === top.flowId) ?? null;
  }, [flows, top]);

  const prLabel = job ? spineLabel(job.artifact) : null;

  // If the currently-selected flow disappears (e.g. after a new PR loads
  // with different flow ids), fall back to PR tab.
  if (top.kind === "flow" && selectedFlow === null && job !== null) {
    setTop({ kind: "pr" });
  }

  return (
    <div className="min-h-screen flex flex-col">
      <TopSpine
        prLabel={prLabel}
        flows={flows}
        top={top}
        onTop={setTop}
        flowSub={flowSub}
        onFlowSub={setFlowSub}
        prSub={prSub}
        onPrSub={setPrSub}
      />
      <main className="flex-1 w-full max-w-6xl mx-auto px-6 pt-4 pb-10">
        {!job ? (
          <LoadForm onJob={setJob} />
        ) : top.kind === "flow" && selectedFlow ? (
          <FlowWorkspace
            artifact={job.artifact}
            jobId={job.jobId}
            flow={selectedFlow}
            sub={flowSub}
          />
        ) : (
          <PrWorkspace
            artifact={job.artifact}
            jobId={job.jobId}
            sub={prSub}
            onTop={(t) => {
              setTop(t);
              // Jump straight to the flow's overview when opening a flow
              // from the flows-map click.
              if (t.kind === "flow") setFlowSub("overview");
            }}
          />
        )}
      </main>
    </div>
  );
}

function spineLabel(a: Artifact): string {
  if (a.pr.repo !== "unknown" && !isPathSha(a.pr.head_sha)) {
    return `${a.pr.repo} · ${shortSha(a.pr.head_sha)}`;
  }
  return deriveSlug(a.pr.base_sha, a.pr.head_sha);
}
