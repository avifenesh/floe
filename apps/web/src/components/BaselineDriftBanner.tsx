import { useEffect, useState } from "react";
import type { Artifact, ArtifactBaseline } from "@/types/artifact";
import { fetchLlmConfig, type LlmConfigView } from "@/api";
import { detectBaselineDrift, type DriftAxis } from "@/lib/baseline-pin";

/** "Re-baseline required" banner — RFC v0.3 §9 UI half.
 *
 *  Compares the artifact's baseline pin against the server's current
 *  LLM regime (`GET /llm-config`). When any model axis has drifted,
 *  the displayed cost / proof numbers were produced by models the
 *  user is no longer running — a re-run would produce different
 *  output.
 *
 *  Renders nothing when:
 *  - the artifact has no baseline yet (probe hasn't run), OR
 *  - `GET /llm-config` is still in flight / errored, OR
 *  - every axis matches.
 *
 *  When the artifact was produced from a known re-runnable source
 *  (sample id in `pr.repo`), the banner offers a "Re-run now"
 *  action. For path-driven and URL-driven runs we only show the
 *  passive warning — the caller has to re-paste.
 */
export function BaselineDriftBanner({
  artifact,
  onRebaseline,
  rebaselining,
}: {
  artifact: Artifact;
  /** Fires when the user clicks Re-run. Caller spawns the fresh
   *  analysis + swaps the open workspace. Leave undefined to hide
   *  the button (warning still shows). */
  onRebaseline?: () => void;
  rebaselining?: boolean;
}) {
  const [cfg, setCfg] = useState<LlmConfigView | null>(null);
  useEffect(() => {
    let cancelled = false;
    fetchLlmConfig()
      .then((c) => {
        if (!cancelled) setCfg(c);
      })
      .catch(() => {
        // Silent on failure — absence of banner is the safe default
        // (don't cry wolf when the endpoint itself is unreachable).
      });
    return () => {
      cancelled = true;
    };
  }, []);

  const baseline: ArtifactBaseline | null | undefined = artifact.baseline;
  if (!baseline || !cfg) return null;
  const drift = detectBaselineDrift(baseline, cfg);
  if (drift.length === 0) return null;

  return (
    <div
      role="alert"
      className="mb-3 rounded-md border border-amber-500/40 bg-amber-500/5 px-3 py-2 text-[12px] font-mono text-foreground"
    >
      <div className="flex items-baseline gap-2 flex-wrap">
        <span className="text-amber-500 font-semibold">re-baseline required</span>
        <span className="text-muted-foreground flex-1 min-w-0">
          the LLM regime has shifted since this analysis ran — the numbers below
          were produced by different models than a re-run would use
        </span>
        {onRebaseline && (
          <button
            onClick={onRebaseline}
            disabled={rebaselining}
            className={
              "shrink-0 inline-flex items-center gap-1.5 text-[11px] font-mono " +
              "rounded-md border border-amber-500/50 bg-amber-500/10 px-2.5 py-1 " +
              "text-amber-900 dark:text-amber-100 hover:bg-amber-500/20 " +
              "disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
            }
          >
            <span aria-hidden>↻</span>
            <span>{rebaselining ? "re-running…" : "Re-run now"}</span>
          </button>
        )}
      </div>
      <ul className="mt-1 space-y-0.5">
        {drift.map((d) => (
          <li key={d.axis} className="flex items-baseline gap-2">
            <span className="uppercase tracking-wide opacity-60 min-w-[72px]">
              {d.axis}
            </span>
            <span className="text-foreground/80">{formatAxis(d)}</span>
          </li>
        ))}
      </ul>
    </div>
  );
}

function formatAxis(d: DriftAxis): string {
  const was = d.was ?? "<skipped>";
  const now = d.now ?? "<skipped>";
  return `was ${was} · now ${now}`;
}
