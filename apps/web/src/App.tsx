import { useEffect, useMemo, useState } from "react";
import { TopSpine } from "@/components/TopSpine";
import { Palette } from "@/components/Palette";
import { LandingPage } from "@/views/landing-page";
import { Dashboard } from "@/views/dashboard";
import { FlowWorkspace } from "@/views/flow-workspace";
import { PrWorkspace } from "@/views/pr-workspace";
import type { FlowSubTab, PrSubTab, TopTab } from "@/views/types";
import type { Artifact } from "@/types/artifact";
import { deriveSlug, isPathSha, shortSha } from "@/lib/artifact";
import { fetchMe, getJob, logout, pollUntilDone, rebaselineJob, type Me } from "@/api";

export interface LoadedJob {
  jobId: string;
  artifact: Artifact;
}

export default function App() {
  const [job, setJob] = useState<LoadedJob | null>(null);
  const [top, setTop] = useState<TopTab>({ kind: "pr" });
  const [flowSub, setFlowSub] = useState<FlowSubTab>("overview");
  const [prSub, setPrSub] = useState<PrSubTab>("flows-map");
  // Auth state lifted here so routing between Landing/Dashboard
  // branches cleanly. `undefined` = not yet fetched (show nothing);
  // `null` = fetched, not signed in; Me = signed in.
  const [me, setMe] = useState<Me | null | undefined>(undefined);

  useEffect(() => {
    void fetchMe()
      .then((m) => setMe(m))
      .catch(() => setMe(null));
  }, []);

  // Progressive disclosure (slice C). Synth + probe + proof run async
  // AFTER the sync pipeline publishes "ready" — the artifact has
  // structural flows + evidence immediately, but flow NAMES come from
  // synth (cluster-parallel GLM) and cost/proof come from probe/proof.
  // Poll every 4s while ANY of those is still analyzing so names,
  // cost deltas, and proof verdicts backfill without a manual refresh.
  useEffect(() => {
    if (!job) return;
    const cost = job.artifact.cost_status ?? "not-run";
    const proof = job.artifact.proof_status ?? "not-run";
    const synth = job.artifact.synth_status ?? "not-run";
    if (
      cost !== "analyzing" &&
      proof !== "analyzing" &&
      synth !== "analyzing"
    ) {
      return;
    }

    let cancelled = false;
    let aborted = false;
    const tick = async () => {
      if (aborted) return;
      try {
        const v = await getJob(job.jobId);
        if (cancelled || !v.artifact) return;
        setJob({ jobId: job.jobId, artifact: v.artifact });
      } catch (e) {
        const msg = String(e);
        if (msg.includes("404")) {
          // Job is gone — server restarted without a DB row, or the
          // cache was wiped. Stop polling and flip the still-
          // analyzing statuses to errored locally so the UI stops
          // spinning. Ready fields stay put.
          aborted = true;
          setJob((prev) => {
            if (!prev) return prev;
            const a = { ...prev.artifact };
            if (a.cost_status === "analyzing") a.cost_status = "errored";
            if (a.proof_status === "analyzing") a.proof_status = "errored";
            if (a.synth_status === "analyzing") a.synth_status = "errored";
            return { jobId: prev.jobId, artifact: a };
          });
          return;
        }
        // Other failures are transient; next tick will try again.
      }
    };
    const t = setInterval(() => void tick(), 4000);
    return () => {
      cancelled = true;
      clearInterval(t);
    };
  }, [job]);

  // Re-baseline: drive /analyze/:id/rebaseline server-side. The
  // server knows whether the source is re-runnable (sample or
  // URL-driven with a cached checkout); path-driven artifacts
  // return a 400 the alert surfaces to the reviewer.
  const [rebaselining, setRebaselining] = useState(false);
  const onRebaseline = job
    ? async () => {
        setRebaselining(true);
        try {
          const id = await rebaselineJob(job.jobId);
          const done = await pollUntilDone(id);
          if (done.artifact) setJob({ jobId: id, artifact: done.artifact });
        } catch (e) {
          alert(`Re-baseline failed: ${String(e)}`);
        } finally {
          setRebaselining(false);
        }
      }
    : undefined;

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

  const anyStructural = flows.some(
    (f) => (f.source as { kind: string }).kind === "structural",
  );

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
      {job && anyStructural && <StructuralBannerStrip />}
      {job && <BackgroundWorkStrip artifact={job.artifact} />}
      <main className="flex-1 w-full min-w-0 max-w-6xl mx-auto px-6 pt-4 pb-10">
        {me === undefined ? (
          // Auth fetch hasn't resolved yet — render nothing (avoids a
          // flash of the landing page on first paint for signed-in
          // users whose cookie is valid).
          <div />
        ) : !job ? (
          me ? (
            <Dashboard
              me={me}
              onJob={setJob}
              onSignOut={async () => {
                await logout();
                setMe(null);
              }}
            />
          ) : (
            <LandingPage onJob={setJob} />
          )
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
            onRebaseline={onRebaseline}
            rebaselining={rebaselining}
          />
        )}
      </main>
      {job && (
        <Palette
          flows={flows}
          top={top}
          onTop={setTop}
          onFlowSub={setFlowSub}
          onPrSub={setPrSub}
        />
      )}
    </div>
  );
}

/** Persistent footer strip showing which post-READY background passes
 *  are still running. Silent when everything is settled. Gives reviewers
 *  a single always-visible place to see "something is still cooking" —
 *  especially important on sub-tabs that would otherwise look empty
 *  (Cost before probe, Proof before GLM finishes, Flow before synth).
 *
 *  Labels describe the WORK, not the model (see feedback memory).
 */
function BackgroundWorkStrip({ artifact }: { artifact: Artifact }) {
  const synth = artifact.synth_status ?? "not-run";
  const cost = artifact.cost_status ?? "not-run";
  const proof = artifact.proof_status ?? "not-run";
  const active = [
    synth === "analyzing" && { label: "Naming flows", hint: "Assigning a human name and rationale to each flow." },
    cost === "analyzing" && { label: "Measuring nav cost", hint: "Probing base + head to score how hard the repo is to navigate." },
    proof === "analyzing" && { label: "Matching flows to intent", hint: "Scoring intent-fit and hunting for proof per claim." },
  ].filter(Boolean) as { label: string; hint: string }[];

  if (active.length === 0) return null;

  return (
    <div className="border-b border-border/60 bg-muted/20">
      <div className="w-full max-w-6xl mx-auto px-6 py-1.5 flex items-center gap-3 text-[11px] font-mono text-muted-foreground">
        <span className="uppercase tracking-wide text-[10px]">Still working</span>
        <ul className="flex items-center gap-2 flex-wrap">
          {active.map((a) => (
            <li
              key={a.label}
              title={a.hint}
              className="inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border border-border/60 bg-background/60"
            >
              <span className="relative inline-flex" aria-hidden>
                <span className="absolute inset-0 w-1.5 h-1.5 rounded-full bg-muted-foreground/40 animate-ping" />
                <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground/80" />
              </span>
              <span className="text-foreground/80">{a.label}</span>
              <span className="text-muted-foreground/70">…</span>
            </li>
          ))}
        </ul>
        <span className="ml-auto text-muted-foreground/70">
          results fill in automatically
        </span>
      </div>
    </div>
  );
}

function StructuralBannerStrip() {
  return (
    <div className="border-y border-amber-300/40 dark:border-amber-400/20 bg-amber-100/70 dark:bg-amber-400/[0.07]">
      <div className="w-full max-w-6xl mx-auto px-6 py-1.5 flex items-center gap-2 text-[11px] font-mono text-amber-900 dark:text-amber-200">
        <span aria-hidden className="inline-block w-1.5 h-1.5 rounded-full bg-amber-500" />
        Structural clustering — LLM synthesis not available. Flows reflect qualified-name prefix, not architectural intent.
      </div>
    </div>
  );
}

function spineLabel(a: Artifact): string {
  if (a.pr.repo !== "unknown" && !isPathSha(a.pr.head_sha)) {
    return `${a.pr.repo} · ${shortSha(a.pr.head_sha)}`;
  }
  return deriveSlug(a.pr.base_sha, a.pr.head_sha);
}
