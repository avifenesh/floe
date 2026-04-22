/** Live pipeline-stage progress while a job is pending.
 *
 *  Subscribes to the server's SSE stream (`/analyze/:id/stream`) and
 *  renders each stage as ✓ done / • running / ○ pending / ✗ errored.
 *  Turns a 5–15 minute wait from dead "Analysing…" text into a visible
 *  intelligence demo: the reviewer watches the pipeline work.
 *
 *  Stages match what the server emits (see worker.rs + probe/intent
 *  pipelines): parse-head → parse-base → cfg → hunks → flows →
 *  evidence → llm-synthesize → ready → probe → proof.
 */

import { useEffect, useMemo, useState } from "react";

interface Props {
  jobId: string;
  /** Backend origin (e.g. `http://127.0.0.1:8787`). Empty string means
   *  same-origin (production build). */
  backendBase: string;
}

type StageState = "pending" | "running" | "done" | "errored";

interface StageRow {
  id: string;
  label: string;
  hint: string;
  state: StageState;
  message?: string;
  /** Millisecond timestamp when this stage first flipped to `running`.
   *  Used to render a live "42s" counter on the active row. */
  startedAt?: number;
  /** Millisecond timestamp when this stage finished (done / errored).
   *  Locks in the elapsed display so completed stages stop ticking. */
  endedAt?: number;
}

const INITIAL_STAGES: Omit<StageRow, "state" | "message">[] = [
  { id: "parse-head", label: "Parse head", hint: "Walk the head worktree." },
  { id: "parse-base", label: "Parse base", hint: "Walk the base worktree." },
  { id: "cfg", label: "Control flow", hint: "Build per-function CFGs." },
  { id: "hunks", label: "Hunks", hint: "Extract semantic hunks." },
  { id: "flows", label: "Flows", hint: "Cluster into architectural stories." },
  { id: "llm-synthesize", label: "Naming flows", hint: "Assign a human name and rationale to each flow." },
  { id: "evidence", label: "Evidence", hint: "Collect deterministic claims." },
  { id: "ready", label: "Ready", hint: "Flows + evidence live — workspace opens." },
  { id: "probe", label: "Nav cost", hint: "Measure how hard the repo is to navigate, base vs head." },
  { id: "proof", label: "Intent & proof", hint: "Match flows to intent, hunt for evidence per claim." },
];

export function PipelineProgress({ jobId, backendBase }: Props) {
  const [stages, setStages] = useState<StageRow[]>(() =>
    INITIAL_STAGES.map((s) => ({ ...s, state: "pending" })),
  );
  const [latestMessage, setLatestMessage] = useState<string>("starting…");
  // Tick every second so the active-stage elapsed counter updates
  // without re-rendering every event — keeps the UI live even during
  // quiet periods of the pipeline (e.g. GLM 60s turn latencies).
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const t = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(t);
  }, []);

  useEffect(() => {
    const url = `${backendBase}/analyze/${jobId}/stream`;
    const es = new EventSource(url, { withCredentials: true });

    const advance = (stageId: string, state: StageState, message?: string) => {
      setStages((prev) => {
        const idx = prev.findIndex((s) => s.id === stageId);
        if (idx === -1) return prev;
        const next = prev.slice();
        const t = Date.now();
        // If this stage starts running, mark all earlier pending stages
        // as done (we may have missed their 'started' events because
        // SSE is live-only, no replay).
        if (state === "running" || state === "done") {
          for (let i = 0; i < idx; i++) {
            if (next[i].state === "pending") {
              next[i] = {
                ...next[i],
                state: "done",
                startedAt: next[i].startedAt ?? t,
                endedAt: t,
              };
            }
          }
        }
        const current = next[idx];
        next[idx] = {
          ...current,
          state,
          message,
          startedAt: current.startedAt ?? (state === "running" ? t : undefined),
          endedAt: state === "done" || state === "errored" ? t : current.endedAt,
        };
        return next;
      });
      if (message) setLatestMessage(message);
    };

    const onEvent = (ev: MessageEvent) => {
      try {
        const payload = JSON.parse(ev.data) as {
          stage: string;
          percent: number;
          message: string;
        };
        const isError = payload.stage === "error";
        if (isError) {
          setStages((prev) =>
            prev.map((s) =>
              s.state === "running"
                ? { ...s, state: "errored", message: payload.message }
                : s,
            ),
          );
          setLatestMessage(`error: ${payload.message}`);
          return;
        }
        // 100% on a stage = done; anything less = running
        const state: StageState = payload.percent >= 100 ? "done" : "running";
        advance(payload.stage, state, payload.message);
      } catch {
        // ignore malformed frames
      }
    };

    es.onmessage = onEvent;
    // Server sends typed events matching stage names — bind a catch-all
    // listener for each.
    for (const s of INITIAL_STAGES) {
      es.addEventListener(s.id, onEvent as EventListener);
    }
    es.addEventListener("error", onEvent as EventListener);

    return () => es.close();
  }, [jobId, backendBase]);

  const activeIdx = stages.findIndex((s) => s.state === "running");
  const erroredIdx = stages.findIndex((s) => s.state === "errored");

  // Total elapsed across the whole run (first stage started → now).
  // Gives the reviewer a headline "I've been waiting 47s" number
  // without doing the math across stages.
  const totalElapsed = useMemo(() => {
    const firstStart = stages.find((s) => s.startedAt)?.startedAt;
    if (!firstStart) return 0;
    return Math.max(0, Math.round((now - firstStart) / 1000));
  }, [stages, now]);

  return (
    <section className="rounded-xl border border-border/60 bg-muted/10 p-5 space-y-4">
      <header className="flex items-baseline justify-between gap-3">
        <div className="flex items-baseline gap-2">
          <h2 className="text-[13px] font-semibold text-foreground">
            Analysis in progress
          </h2>
          {totalElapsed > 0 && (
            <span className="text-[11px] font-mono text-muted-foreground tabular-nums">
              {formatElapsed(totalElapsed)}
            </span>
          )}
        </div>
        <p className="text-[11px] font-mono text-muted-foreground truncate max-w-[60%]">
          {latestMessage}
        </p>
      </header>
      <ol className="space-y-0">
        {stages.map((s, i) => {
          const isActive = i === activeIdx;
          const isErrored = i === erroredIdx;
          const elapsed = stageElapsed(s, now);
          return (
            <li
              key={s.id}
              className={`grid grid-cols-[1.5rem,1fr] gap-3 py-2 px-3 border-l-2 rounded-r transition-colors ${
                isActive
                  ? "border-foreground bg-foreground/5"
                  : isErrored
                    ? "border-destructive bg-destructive/5"
                    : s.state === "done"
                      ? "border-muted-foreground/20"
                      : "border-border/30"
              }`}
            >
              <StageIcon state={s.state} />
              <div className="min-w-0">
                <div className="flex items-baseline gap-2">
                  <span
                    className={`text-[13px] ${
                      s.state === "done"
                        ? "text-muted-foreground/70"
                        : isActive
                          ? "text-foreground font-semibold"
                          : "text-foreground"
                    }`}
                  >
                    {s.label}
                  </span>
                  {isActive && (
                    <span className="text-[10px] font-mono uppercase tracking-wide text-foreground animate-pulse">
                      running
                    </span>
                  )}
                  {isErrored && (
                    <span className="text-[10px] font-mono uppercase tracking-wide text-destructive">
                      errored
                    </span>
                  )}
                  {elapsed !== null && (
                    <span className="ml-auto text-[10px] font-mono text-muted-foreground tabular-nums">
                      {formatElapsed(elapsed)}
                    </span>
                  )}
                </div>
                <p className="text-[11px] text-muted-foreground leading-snug truncate">
                  {s.message ?? s.hint}
                </p>
              </div>
            </li>
          );
        })}
      </ol>
      <p className="text-[10px] font-mono text-muted-foreground pt-2 border-t border-border/40">
        Nav cost + intent & proof run in parallel after flows are ready — verdicts stream in as they complete.
      </p>
    </section>
  );
}

/** Elapsed seconds for a stage, or null if it never started. Active
 *  stages keep ticking via `now`; done/errored freeze at `endedAt`.
 */
function stageElapsed(s: StageRow, now: number): number | null {
  if (!s.startedAt) return null;
  const end = s.endedAt ?? now;
  return Math.max(0, Math.round((end - s.startedAt) / 1000));
}

/** "7s" / "1:23" / "12:04". Minute formatting kicks in at 60s so the
 *  headline counter reads cleanly during long pipeline runs.
 */
function formatElapsed(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60);
  const s = seconds % 60;
  return `${m}:${s.toString().padStart(2, "0")}`;
}

function StageIcon({ state }: { state: StageState }) {
  if (state === "done") {
    return (
      <span
        className="mt-[2px] inline-flex items-center justify-center w-5 h-5 rounded-full bg-muted-foreground/20 text-muted-foreground text-[11px]"
        aria-label="done"
      >
        ✓
      </span>
    );
  }
  if (state === "running") {
    // Beefier active cue — 3-layer pulse: outer ping ring, middle
    // slower pulse, solid dot. Reads "alive" at a glance.
    return (
      <span className="mt-[2px] inline-flex items-center justify-center w-5 h-5">
        <span className="relative inline-flex w-3.5 h-3.5 items-center justify-center">
          <span className="absolute inset-0 rounded-full bg-foreground/40 animate-ping" />
          <span className="absolute inset-[2px] rounded-full bg-foreground/60 animate-pulse" />
          <span className="relative rounded-full bg-foreground w-1.5 h-1.5" />
        </span>
      </span>
    );
  }
  if (state === "errored") {
    return (
      <span
        className="mt-[2px] inline-flex items-center justify-center w-5 h-5 rounded-full bg-destructive/20 text-destructive text-[11px]"
        aria-label="errored"
      >
        ✗
      </span>
    );
  }
  return (
    <span
      className="mt-[2px] inline-flex items-center justify-center w-5 h-5 rounded-full border border-border/60 text-muted-foreground/60 text-[11px]"
      aria-label="pending"
    >
      ○
    </span>
  );
}
