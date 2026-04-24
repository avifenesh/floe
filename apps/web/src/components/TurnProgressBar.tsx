import { useEffect, useRef, useState } from "react";
import { fetchProgress, type TurnProgressView } from "@/api";

/** Turn-count progress bar with shimmer.
 *
 *  - Floor = `(current - 1) / max` percent, advances one decile per
 *    confirmed turn.
 *  - Between turns the visual creeps asymptotically toward the next
 *    decile via rAF easing — progress feels continuous even though
 *    the server only confirms at turn boundaries.
 *  - A diagonal shimmer runs across the fill regardless of motion so
 *    the bar reads as *alive* when waiting on the LLM.
 *  - Pulse flash when a new turn is confirmed.
 *  - `stuck Xs` label after `stuckAfterMs` without a turn advance. */
export function TurnProgressBar({
  jobId,
  passKey,
  complete,
  label,
  stuckAfterMs = 30_000,
}: {
  jobId: string;
  passKey: string;
  complete: boolean;
  label?: string;
  stuckAfterMs?: number;
}) {
  const [progress, setProgress] = useState<TurnProgressView | null>(null);
  const [visualPct, setVisualPct] = useState(0);
  const [pulseKey, setPulseKey] = useState(0);
  const lastAdvance = useRef<number>(Date.now());
  const rafRef = useRef<number | null>(null);
  // Forced re-render every 500ms so the "stuck Xs" label ticks
  // without needing external state updates.
  const [, setTick] = useState(0);

  // Poll the server every 2s.
  useEffect(() => {
    if (complete) return;
    let cancelled = false;
    const tick = async () => {
      try {
        const all = await fetchProgress(jobId);
        if (cancelled) return;
        const p = all[passKey];
        if (p) {
          setProgress((prev) => {
            if (!prev || prev.current !== p.current) {
              lastAdvance.current = Date.now();
              setPulseKey((k) => k + 1);
            }
            return p;
          });
        }
      } catch {
        /* transient */
      }
    };
    void tick();
    const t = setInterval(() => void tick(), 2000);
    const stuckTicker = setInterval(() => setTick((x) => x + 1), 500);
    return () => {
      cancelled = true;
      clearInterval(t);
      clearInterval(stuckTicker);
    };
  }, [jobId, passKey, complete]);

  // rAF interpolation.
  useEffect(() => {
    const step = () => {
      setVisualPct((prev) => {
        const target = complete
          ? 100
          : progress
            ? Math.min(
                100,
                ((progress.current - 1 + 0.92) / Math.max(1, progress.max)) * 100,
              )
            : 3;
        const delta = target - prev;
        if (Math.abs(delta) < 0.05) return target;
        return prev + delta * 0.05;
      });
      rafRef.current = requestAnimationFrame(step);
    };
    rafRef.current = requestAnimationFrame(step);
    return () => {
      if (rafRef.current) cancelAnimationFrame(rafRef.current);
    };
  }, [progress, complete]);

  const pct = Math.max(3, Math.round(visualPct));
  const age = progress
    ? Math.round((Date.now() - lastAdvance.current) / 1000)
    : 0;
  const stuck = !complete && progress !== null && age * 1000 > stuckAfterMs;
  const fillTone = complete
    ? "from-emerald-400/70 to-emerald-500/80"
    : stuck
      ? "from-amber-400/70 to-amber-500/80"
      : "from-sky-400/70 to-indigo-500/80";

  return (
    <div className="space-y-1.5">
      <div className="flex items-baseline justify-between text-[10px] font-mono">
        <span className="text-muted-foreground">
          {label ?? passKey}
        </span>
        <span className="tabular-nums text-muted-foreground">
          {progress ? (
            <>
              <span className="text-foreground font-medium">
                turn {progress.current}/{progress.max}
              </span>
              <span className="mx-1.5 opacity-50">·</span>
              <span>{pct}%</span>
            </>
          ) : complete ? (
            <span className="text-foreground font-medium">done</span>
          ) : (
            <span className="inline-flex items-baseline gap-1">
              <span className="floe-dot-pulse" aria-hidden>
                •
              </span>
              <span>waiting</span>
            </span>
          )}
          {stuck && (
            <span className="ml-2 text-amber-600 dark:text-amber-400 font-medium">
              · stuck {age}s
            </span>
          )}
        </span>
      </div>
      <div
        className="relative h-2 rounded-full bg-muted/70 overflow-hidden"
        role="progressbar"
        aria-valuenow={pct}
        aria-valuemin={0}
        aria-valuemax={100}
      >
        {/* Base fill with gradient. */}
        <div
          className={
            "absolute inset-y-0 left-0 rounded-full bg-gradient-to-r " +
            fillTone
          }
          style={{ width: `${pct}%` }}
        />
        {/* Moving diagonal shimmer on top. Runs constantly while
            not complete so the bar reads as active even when the
            progress value is steady between turns. */}
        {!complete && (
          <div
            className="absolute inset-y-0 left-0 floe-shimmer rounded-full"
            style={{ width: `${pct}%` }}
          />
        )}
        {/* Pulse overlay — fires once per turn advance. The `key`
            re-mount restarts the animation. */}
        <div
          key={pulseKey}
          className={pulseKey > 0 ? "absolute inset-0 floe-pulse-flash" : ""}
          style={{ width: `${pct}%` }}
        />
      </div>
    </div>
  );
}
