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
import { useToast } from "@/components/Toast";

export interface LoadedJob {
  jobId: string;
  artifact: Artifact;
}

export default function App() {
  const toast = useToast();
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
          // READY fires as soon as structural+evidence land. Synth,
          // probe, proof and summary keep running in the background;
          // keep polling until all of them leave `analyzing` so the
          // drift banner + cost/proof sections reflect the fresh run
          // (otherwise "Re-run now" looks like a no-op because the
          // pinned models haven't been stamped yet).
          let done = await pollUntilDone(id);
          if (done.artifact) setJob({ jobId: id, artifact: done.artifact });
          const stillRunning = (a: typeof done.artifact) =>
            !!a && (
              a.synth_status === "analyzing" ||
              a.cost_status === "analyzing" ||
              a.proof_status === "analyzing"
            );
          // Bounded wait — up to ~3 min at 1s intervals.
          for (let i = 0; i < 180 && stillRunning(done.artifact); i++) {
            await new Promise((r) => setTimeout(r, 1000));
            done = await getJob(id);
            if (done.artifact) setJob({ jobId: id, artifact: done.artifact });
          }
        } catch (e) {
          toast.push({
            title: "Re-baseline failed",
            body: String(e),
            tone: "error",
          });
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
        onHome={() => {
          setJob(null);
          setTop({ kind: "pr" });
          setPrSub("flows-map");
          setFlowSub("overview");
        }}
      />
      {job && anyStructural && <StructuralBannerStrip />}
      {job && <BackgroundWorkStrip artifact={job.artifact} />}
      <main className="flex-1 w-full min-w-0 max-w-7xl mx-auto px-6 pt-4 pb-10">
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
            onInlineNotesChange={(next) =>
              setJob((prev) =>
                prev
                  ? { ...prev, artifact: { ...prev.artifact, inline_notes: next } }
                  : prev,
              )
            }
            onJumpToSource={(entity) => {
              setFlowSub("source");
              if (entity) {
                // DiffView's FileSidebar scrolls the right file into view
                // on selection. The NodeDetailPanel can also be spawned
                // from here; for now just flip the sub-tab — the
                // file list is already scoped to the flow's entities.
                window.requestAnimationFrame(() => {
                  const el = document.querySelector<HTMLElement>(
                    `[data-entity-name="${CSS.escape(entity)}"]`,
                  );
                  el?.scrollIntoView({ behavior: "smooth", block: "center" });
                });
              }
            }}
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
            onInlineNotesChange={(next) =>
              setJob((prev) =>
                prev
                  ? { ...prev, artifact: { ...prev.artifact, inline_notes: next } }
                  : prev,
              )
            }
            onArtifactChange={(next) =>
              setJob((prev) => (prev ? { ...prev, artifact: next } : prev))
            }
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
  type Status = "ready" | "analyzing" | "not-run" | "errored";
  const stages: Array<{ key: string; label: string; status: Status; hint: string }> = [
    {
      key: "structural",
      label: "Structural flows",
      status: "ready",
      hint: "Deterministic floor — always green once the artifact lands.",
    },
    {
      key: "synth",
      label: "LLM flow names",
      status: (artifact.synth_status ?? "not-run") as Status,
      hint: "Assigning a human name and rationale to each flow.",
    },
    {
      key: "cost",
      label: "Nav cost probe",
      status: (artifact.cost_status ?? "not-run") as Status,
      hint: "Probing base + head to score how hard the repo is to navigate.",
    },
    {
      key: "proof",
      label: "Intent & proof",
      status: (artifact.proof_status ?? "not-run") as Status,
      hint: "Scoring intent-fit and hunting for proof per claim.",
    },
  ];

  const anyActive = stages.some((s) => s.status === "analyzing");
  const anyDone = stages.some((s) => s.status === "ready" && s.key !== "structural");
  const anyError = stages.some((s) => s.status === "errored");
  // Hide the strip if nothing interesting is running and there's
  // nothing unusual to report — avoids a permanent chrome strip on a
  // fully-static PR without LLM passes configured.
  if (!anyActive && !anyDone && !anyError) return null;

  return (
    <div className="border-b border-border/60 bg-muted/20">
      <div className="w-full max-w-7xl mx-auto px-6 py-1.5 flex items-center gap-3 text-[11px] font-mono text-muted-foreground">
        <span className="uppercase tracking-wide text-[10px]">Pipeline</span>
        <ul className="flex items-center gap-2 flex-wrap">
          {stages.map((s, i) => (
            <li
              key={s.key}
              title={s.hint}
              className="inline-flex items-center gap-1.5"
            >
              <span className={stageChipClass(s.status)}>
                <StageGlyph status={s.status} />
                <span>{s.label}</span>
              </span>
              {i < stages.length - 1 && (
                <span aria-hidden className="text-muted-foreground/50">→</span>
              )}
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function stageChipClass(
  status: "ready" | "analyzing" | "not-run" | "errored",
): string {
  const base =
    "inline-flex items-center gap-1.5 px-2 py-0.5 rounded-full border transition-colors ";
  switch (status) {
    case "ready":
      return (
        base +
        "border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200"
      );
    case "analyzing":
      return base + "border-border/60 bg-background/60 text-foreground/80";
    case "errored":
      return (
        base +
        "border-rose-400/40 bg-rose-50 text-rose-800 dark:bg-rose-400/10 dark:text-rose-200"
      );
    case "not-run":
    default:
      return base + "border-border/40 bg-transparent text-muted-foreground/70";
  }
}

function StageGlyph({
  status,
}: {
  status: "ready" | "analyzing" | "not-run" | "errored";
}) {
  if (status === "analyzing") {
    return (
      <span className="relative inline-flex" aria-hidden>
        <span className="absolute inset-0 w-1.5 h-1.5 rounded-full bg-muted-foreground/40 animate-ping" />
        <span className="w-1.5 h-1.5 rounded-full bg-muted-foreground/80" />
      </span>
    );
  }
  if (status === "ready") {
    return <span aria-hidden className="text-[10px] leading-none">✓</span>;
  }
  if (status === "errored") {
    return <span aria-hidden className="text-[10px] leading-none">!</span>;
  }
  return <span aria-hidden className="w-1.5 h-1.5 rounded-full border border-current/60" />;
}

function StructuralBannerStrip() {
  return (
    <div className="border-y border-amber-300/40 dark:border-amber-400/20 bg-amber-100/70 dark:bg-amber-400/[0.07]">
      <div className="w-full max-w-7xl mx-auto px-6 py-1.5 flex items-center gap-2 text-[11px] font-mono text-amber-900 dark:text-amber-200">
        <span aria-hidden className="inline-block w-1.5 h-1.5 rounded-full bg-amber-500" />
        Structural clustering — LLM synthesis not available. Flows reflect qualified-name prefix, not architectural intent.
      </div>
    </div>
  );
}

function spineLabel(a: Artifact): string {
  // LLM-derived headline wins when present — reviewer sees what the PR
  // IS, not just where it lives.
  if (a.pr_summary?.headline) {
    return a.pr_summary.headline;
  }
  if (a.pr.repo !== "unknown" && !isPathSha(a.pr.head_sha)) {
    return `${a.pr.repo} · ${shortSha(a.pr.head_sha)}`;
  }
  return deriveSlug(a.pr.base_sha, a.pr.head_sha);
}
