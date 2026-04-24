import { useState } from "react";
import type { Artifact } from "@/types/artifact";
import { retryErroredAxes } from "@/api";

/** Banner surfaced when one or more LLM axes errored mid-pipeline —
 *  typically a token-budget abort. Offers a "retry errored" button
 *  that re-runs only the failed axes, keeping the data that did land
 *  from the successful ones.
 *
 *  Distinct from `BaselineDriftBanner`, which fires on model-pin
 *  mismatch. Both can be visible simultaneously. */
export function ErroredAxesBanner({
  jobId,
  artifact,
  onRetryStarted,
}: {
  jobId: string;
  artifact: Artifact;
  /** Fires after the retry POST returns. Parent should flip any
   *  local spinners + begin polling for fresh artifact state. */
  onRetryStarted?: (retried: string[]) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const erroredAxes: string[] = [];
  if (artifact.proof_status === "errored") erroredAxes.push("proof / intent-fit");
  if (artifact.cost_status === "errored") erroredAxes.push("cost");
  // Membership: flag if proof ran (status != not-run) but any flow
  // lacks membership. That's the "session ended without final
  // content" case we see in logs.
  const missingMembership =
    artifact.proof_status !== "not-run" &&
    (artifact.flows ?? []).some((f) => !f.membership);
  if (missingMembership) erroredAxes.push("membership");
  if (erroredAxes.length === 0) return null;

  const retry = async () => {
    setBusy(true);
    setErr(null);
    try {
      const retried = await retryErroredAxes(jobId);
      onRetryStarted?.(retried);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div
      role="alert"
      className="mb-3 rounded-md border border-rose-400/40 bg-rose-500/5 px-3 py-2 text-[12px] font-mono text-foreground shadow-sm"
    >
      <div className="flex items-baseline gap-2 flex-wrap">
        <span className="text-rose-500 font-semibold">axes errored</span>
        <span className="text-muted-foreground flex-1 min-w-0">
          {erroredAxes.join(" · ")}
          {" "}— usually a token budget abort. Retry re-runs only the
          errored axes, keeps what succeeded.
        </span>
        <button
          onClick={retry}
          disabled={busy}
          className={
            "shrink-0 inline-flex items-center gap-1.5 text-[11px] font-mono " +
            "rounded-md border border-rose-500/50 bg-rose-500/10 px-2.5 py-1 " +
            "text-rose-900 dark:text-rose-100 hover:bg-rose-500/20 " +
            "disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          }
        >
          <span aria-hidden>↻</span>
          <span>{busy ? "retrying…" : "Retry errored"}</span>
        </button>
      </div>
      {err && (
        <p className="mt-1 text-[11px] text-rose-700 dark:text-rose-300/90">
          {err}
        </p>
      )}
    </div>
  );
}
