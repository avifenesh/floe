/** Public landing page — shown only to unauthenticated visitors.
 *  The whole page is the sell: tagline, three-bullet explainer,
 *  sample gallery, Sign-in primary CTA.
 *
 *  No form, no sidebar — signed-in users never see this view
 *  (App.tsx routes them to <Dashboard /> instead).
 */

import { useEffect, useRef, useState } from "react";
import {
  analyzeSample,
  devLogin,
  githubLoginUrl,
  pollUntilDone,
  type JobView,
} from "@/api";
import type { LoadedJob } from "@/App";
import { LandingHero } from "@/views/landing-hero";
import { PipelineProgress } from "@/views/pipeline-progress";
import { SamplesGallery } from "@/views/samples-gallery";

const PIPELINE_BACKEND =
  typeof window !== "undefined" && window.location.port === "5173"
    ? "http://127.0.0.1:8787"
    : "";

interface Props {
  onJob: (j: LoadedJob | null) => void;
}

/** Dev-only sign-in panel. Only renders on localhost (so a deployed
 *  instance never shows it even by accident). Posts to the
 *  `FLOE_ALLOW_DEV_LOGIN`-gated server route; surfaces the gate's
 *  error if the env var isn't set. After success, hard-reload so
 *  App.tsx re-runs `fetchMe` and routes to the dashboard. */
function DevLoginPanel() {
  const [handle, setHandle] = useState("dev");
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const isLocal =
    typeof window !== "undefined" &&
    /^(localhost|127\.0\.0\.1)$/.test(window.location.hostname);
  if (!isLocal) return null;
  async function go() {
    setBusy(true);
    setErr(null);
    try {
      await devLogin(handle.trim());
      window.location.reload();
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }
  return (
    <details className="mt-4 mx-auto max-w-md rounded border border-border/50 bg-muted/10 px-3 py-2 text-[11px] font-mono">
      <summary className="cursor-pointer select-none text-muted-foreground hover:text-foreground">
        dev sign-in (local only)
      </summary>
      <div className="mt-2 flex items-center gap-2">
        <input
          value={handle}
          onChange={(e) => setHandle(e.target.value)}
          placeholder="handle"
          className="flex-1 border rounded px-2 py-1 bg-background"
        />
        <button
          onClick={() => void go()}
          disabled={busy || !handle.trim()}
          className="px-3 py-1 rounded border border-foreground/70 bg-foreground/90 text-background hover:bg-foreground disabled:opacity-40"
        >
          {busy ? "…" : "sign in"}
        </button>
      </div>
      {err && (
        <p className="mt-1.5 text-rose-600 dark:text-rose-300">{err}</p>
      )}
    </details>
  );
}

export function LandingPage({ onJob }: Props) {
  const [pendingJobId, setPendingJobId] = useState<string | null>(null);
  const [activeSampleId, setActiveSampleId] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  // Track the live job view for progress surface even before the
  // pipeline finishes.
  const [_job, _setJob] = useState<JobView | null>(null);
  const galleryRef = useRef<HTMLDivElement | null>(null);

  async function trySample(sampleId: string) {
    setErr(null);
    setActiveSampleId(sampleId);
    _setJob({ status: "pending" });
    try {
      const id = await analyzeSample(sampleId);
      setPendingJobId(id);
      const done = await pollUntilDone(id);
      _setJob(done);
      setPendingJobId(null);
      setActiveSampleId(null);
      if (done.artifact) onJob({ jobId: id, artifact: done.artifact });
    } catch (e) {
      setErr(String(e));
      setPendingJobId(null);
      setActiveSampleId(null);
      _setJob(null);
    }
  }

  function focusGallery() {
    galleryRef.current?.scrollIntoView({ behavior: "smooth", block: "start" });
  }

  // Keep the progress strip visible while a sample is spinning up;
  // scroll it into view so the reviewer sees what the click did
  // without having to hunt.
  useEffect(() => {
    if (pendingJobId) galleryRef.current?.scrollIntoView({ behavior: "smooth" });
  }, [pendingJobId]);

  return (
    <div className="space-y-6 max-w-7xl mx-auto">
      <LandingHero
        signedIn={false}
        githubLoginUrl={githubLoginUrl}
        onTrySample={focusGallery}
      />

      <div ref={galleryRef}>
        <SamplesGallery
          onPick={(id) => void trySample(id)}
          disabled={pendingJobId !== null}
          activeId={activeSampleId}
        />
      </div>

      {pendingJobId && (
        <PipelineProgress
          jobId={pendingJobId}
          backendBase={PIPELINE_BACKEND}
        />
      )}
      {err && !pendingJobId && (
        <section className="text-[12px] font-mono text-destructive border border-destructive/40 rounded px-3 py-2">
          {err}
        </section>
      )}
      {!pendingJobId && !err && (
        <p className="text-[12px] text-muted-foreground text-center">
          Sign in above to analyse your own PRs — or pick a sample to see a
          finished analysis without signing in.
        </p>
      )}
      <DevLoginPanel />
    </div>
  );
}
