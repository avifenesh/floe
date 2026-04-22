/** Public landing page — shown only to unauthenticated visitors.
 *  The whole page is the sell: tagline, three-bullet explainer,
 *  sample flow preview, Sign-in primary CTA, Try secondary CTA.
 *  No form, no sidebar — signed-in users never see this view
 *  (App.tsx routes them to <Dashboard /> instead).
 */

import { useState } from "react";
import {
  analyze,
  devLogin,
  githubLoginUrl,
  pollUntilDone,
  type JobView,
} from "@/api";
import type { LoadedJob } from "@/App";
import { LandingHero } from "@/views/landing-hero";
import { PipelineProgress } from "@/views/pipeline-progress";

const SAMPLE_BASE = "C:/Users/avife/AppData/Local/Temp/glide-mq-base-181";
const SAMPLE_HEAD = "C:/Users/avife/AppData/Local/Temp/glide-mq-head-181";

const PIPELINE_BACKEND =
  typeof window !== "undefined" && window.location.port === "5173"
    ? "http://127.0.0.1:8787"
    : "";

interface Props {
  onJob: (j: LoadedJob | null) => void;
}

/** Dev-only sign-in panel. Only renders on localhost (so a deployed
 *  instance never shows it even by accident). Posts to the
 *  `ADR_ALLOW_DEV_LOGIN`-gated server route; surfaces the gate's
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
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  // Track the live job view so we can surface progress even before the
  // pipeline finishes — the Try button kicks off a real analysis.
  const [_job, _setJob] = useState<JobView | null>(null);

  async function trySample() {
    setBusy(true);
    setErr(null);
    _setJob({ status: "pending" });
    try {
      const id = await analyze(SAMPLE_BASE, SAMPLE_HEAD);
      setPendingJobId(id);
      const done = await pollUntilDone(id);
      _setJob(done);
      setPendingJobId(null);
      if (done.artifact) onJob({ jobId: id, artifact: done.artifact });
    } catch (e) {
      setErr(String(e));
      setPendingJobId(null);
      _setJob(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-6 max-w-6xl mx-auto">
      <LandingHero
        signedIn={false}
        githubLoginUrl={githubLoginUrl}
        onTrySample={() => void trySample()}
      />

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
          Sign in above to analyse your own PRs — or click{" "}
          <em>Try on glide-mq #181</em> to see a finished analysis.
        </p>
      )}
      <DevLoginPanel />

      {busy && (
        <p className="text-[10px] font-mono text-muted-foreground text-center">
          {busy && pendingJobId
            ? "Analysing — workspace opens when ready."
            : ""}
        </p>
      )}
    </div>
  );
}
