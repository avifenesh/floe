import { useEffect, useState } from "react";
import type { ArtifactBaseline } from "@/types/artifact";
import { fetchLlmConfig, type LlmConfigView } from "@/api";
import { detectBaselineDrift, type DriftAxis } from "@/lib/baseline-pin";

/** "Re-baseline required" banner — RFC v0.3 §9 UI half.
 *
 *  Compares the artifact's baseline pin against the server's current
 *  LLM regime (`GET /llm-config`). When any model axis has drifted,
 *  the displayed cost / proof numbers were produced by models the
 *  user is no longer running — a re-run would produce different
 *  output. Surface it prominently so the reviewer doesn't
 *  silently trust stale numbers.
 *
 *  Renders nothing when:
 *  - the artifact has no baseline yet (probe hasn't run), OR
 *  - `GET /llm-config` is still in flight / errored, OR
 *  - every axis matches.
 *
 *  The matching half of the contract — the full pin readout showing
 *  what produced these numbers — lives in `BaselineStamp` inside
 *  `pr-workspace.tsx`. This banner fires only on drift.
 */
export function BaselineDriftBanner({
  baseline,
}: {
  baseline: ArtifactBaseline | null | undefined;
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

  if (!baseline || !cfg) return null;
  const drift = detectBaselineDrift(baseline, cfg);
  if (drift.length === 0) return null;

  return (
    <div
      role="alert"
      className="mb-3 rounded-md border border-amber-500/40 bg-amber-500/5 px-3 py-2 text-[12px] font-mono text-foreground"
    >
      <div className="flex items-baseline gap-2">
        <span className="text-amber-500 font-semibold">re-baseline required</span>
        <span className="text-muted-foreground">
          the LLM regime has shifted since this analysis ran — the numbers below
          were produced by different models than a re-run would use
        </span>
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
