/** Dashboard — the page signed-in users land on. No marketing hero;
 *  the user is already sold. Two panels: a primary **Analyse a PR**
 *  card where they start a new run, and a **Recent PRs** sidebar
 *  listing their history with dismiss / retry / open actions.
 *
 *  When a run is in flight we collapse the analyse form and surface
 *  the live <PipelineProgress> stage list in the same column so the
 *  user watches the pipeline happen and the sidebar stays put.
 */

import { useCallback, useEffect, useState } from "react";
import {
  analyze,
  analyzeUrl,
  deleteAnalysis,
  getJob,
  listPrAnalyses,
  pollUntilDone,
  type AnalysisRow,
  type Me,
} from "@/api";
import type { LoadedJob } from "@/App";
import { PipelineProgress } from "@/views/pipeline-progress";
import { CompareView } from "@/views/compare-view";

const PIPELINE_BACKEND =
  typeof window !== "undefined" && window.location.port === "5173"
    ? "http://127.0.0.1:8787"
    : "";

interface Props {
  me: Me;
  onSignOut: () => void;
  onJob: (j: LoadedJob | null) => void;
}

export function Dashboard({ me, onSignOut, onJob }: Props) {
  const [base, setBase] = useState(localStorage.getItem("floe.base") ?? "");
  const [head, setHead] = useState(localStorage.getItem("floe.head") ?? "");
  const [prUrl, setPrUrl] = useState("");
  const [intent, setIntent] = useState<unknown>(undefined);
  const [intentLabel, setIntentLabel] = useState<string | null>(null);
  const [intentErr, setIntentErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [err, setErr] = useState<string | null>(null);
  const [history, setHistory] = useState<AnalysisRow[]>([]);
  const [pendingJobId, setPendingJobId] = useState<string | null>(null);

  const refreshHistory = useCallback(async () => {
    try {
      setHistory(await listPrAnalyses(30));
    } catch (e) {
      console.warn("history fetch failed", e);
    }
  }, []);

  useEffect(() => {
    void refreshHistory();
  }, [refreshHistory]);

  // Poll the list every 5s while any row is pending so the chip
  // flips without a manual refresh.
  useEffect(() => {
    if (!history.some((r) => r.status === "pending")) return;
    const t = setInterval(() => void refreshHistory(), 5000);
    return () => clearInterval(t);
  }, [history, refreshHistory]);

  async function runUrl() {
    const url = prUrl.trim();
    if (!url) return;
    setBusy(true);
    setErr(null);
    try {
      const { job_id } = await analyzeUrl(url);
      setPendingJobId(job_id);
      void refreshHistory();
      const done = await pollUntilDone(job_id);
      setPendingJobId(null);
      void refreshHistory();
      if (done.artifact) onJob({ jobId: job_id, artifact: done.artifact });
      else onJob(null);
    } catch (e) {
      setErr(String(e));
      setPendingJobId(null);
      onJob(null);
    } finally {
      setBusy(false);
    }
  }

  async function runLocal() {
    setBusy(true);
    setErr(null);
    localStorage.setItem("floe.base", base);
    localStorage.setItem("floe.head", head);
    try {
      const id = await analyze(base, head, intent);
      setPendingJobId(id);
      void refreshHistory();
      const done = await pollUntilDone(id);
      setPendingJobId(null);
      void refreshHistory();
      if (done.artifact) onJob({ jobId: id, artifact: done.artifact });
      else onJob(null);
    } catch (e) {
      setErr(String(e));
      setPendingJobId(null);
      onJob(null);
    } finally {
      setBusy(false);
    }
  }

  async function openHistory(row: AnalysisRow) {
    if (row.status !== "ready") return;
    setBusy(true);
    setErr(null);
    try {
      const v = await getJob(row.id);
      if (v.artifact) onJob({ jobId: row.id, artifact: v.artifact });
      else setErr(`job ${row.id.slice(0, 8)} has no artifact in memory`);
    } catch (e) {
      setErr(String(e));
    } finally {
      setBusy(false);
    }
  }

  async function dismiss(row: AnalysisRow) {
    setHistory((prev) => prev.filter((r) => r.id !== row.id));
    try {
      await deleteAnalysis(row.id);
    } catch (e) {
      console.warn("dismiss failed", e);
      void refreshHistory();
    }
  }

  async function retry(row: AnalysisRow) {
    if (!base || !head) {
      setErr(
        "Retry needs base/head paths filled in under Local paths below.",
      );
      return;
    }
    await dismiss(row);
    await runLocal();
  }

  const hasPending = history.some((r) => r.status === "pending");
  const [analyseOpen, setAnalyseOpen] = useState(false);
  const readyRows = history.filter((r) => r.status === "ready");
  const pendingRows = history.filter((r) => r.status === "pending");
  const erroredRows = history.filter((r) => r.status === "errored");
  const resume = readyRows[0] ?? null;

  return (
    <div className="max-w-5xl mx-auto space-y-5">
      <TopBar
        me={me}
        onSignOut={onSignOut}
        analyseOpen={analyseOpen}
        onToggleAnalyse={() => setAnalyseOpen((x) => !x)}
        canToggleAnalyse={!pendingJobId}
      />

      {pendingJobId ? (
        <PipelineProgress
          jobId={pendingJobId}
          backendBase={PIPELINE_BACKEND}
        />
      ) : analyseOpen ? (
        <AnalyseCard
          base={base}
          head={head}
          prUrl={prUrl}
          busy={busy}
          onBase={setBase}
          onHead={setHead}
          onPrUrl={setPrUrl}
          onAnalyse={() => void runLocal()}
          onAnalyseUrl={() => void runUrl()}
          intentLabel={intentLabel}
          intentErr={intentErr}
          onIntentFile={async (f) => {
            setIntentErr(null);
            if (!f) {
              setIntent(undefined);
              setIntentLabel(null);
              return;
            }
            try {
              const text = await f.text();
              const parsed = JSON.parse(text);
              setIntent(parsed);
              setIntentLabel(f.name);
            } catch (e) {
              setIntentErr(`invalid intent.json — ${String(e)}`);
              setIntent(undefined);
              setIntentLabel(null);
            }
          }}
        />
      ) : null}

      {err && <ErrorCard raw={err} onDismiss={() => setErr(null)} />}

      <StatsStrip
        ready={readyRows.length}
        pending={pendingRows.length}
        errored={erroredRows.length}
        polling={hasPending}
        onRefresh={() => void refreshHistory()}
      />

      {resume && !analyseOpen && !pendingJobId && (
        <ResumeChip row={resume} onOpen={() => void openHistory(resume)} />
      )}

      <Feed
        history={history}
        onOpen={(r) => void openHistory(r)}
        onDismiss={(r) => void dismiss(r)}
        onRetry={(r) => void retry(r)}
      />
    </div>
  );
}

/**
 * Compact top bar for the dashboard. Replaces the "Welcome back"
 * hero — returning reviewers don't need a greeting; they need the
 * primary action (+ Analyse) and a way out (sign out) without the
 * page spending vertical space on it.
 */
function TopBar({
  me,
  onSignOut,
  analyseOpen,
  onToggleAnalyse,
  canToggleAnalyse,
}: {
  me: Me;
  onSignOut: () => void;
  analyseOpen: boolean;
  onToggleAnalyse: () => void;
  canToggleAnalyse: boolean;
}) {
  return (
    <header className="flex items-center justify-between gap-3">
      <button
        onClick={onToggleAnalyse}
        disabled={!canToggleAnalyse}
        className="inline-flex items-center gap-2 text-[13px] font-medium rounded-md border border-foreground/80 bg-foreground text-background px-3.5 py-1.5 hover:bg-foreground/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
      >
        <span aria-hidden className="text-[14px] leading-none">
          {analyseOpen ? "×" : "+"}
        </span>
        <span>{analyseOpen ? "Close" : "Analyse PR"}</span>
      </button>
      <div className="flex items-center gap-2">
        {me.avatar_url && (
          <img
            src={me.avatar_url}
            alt=""
            className="w-7 h-7 rounded-full border border-border/60"
          />
        )}
        <span className="text-[12px] font-mono text-foreground">
          {me.display_name ?? me.provider_user_id}
        </span>
        <button
          onClick={onSignOut}
          className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
          title="Sign out"
        >
          sign out
        </button>
      </div>
    </header>
  );
}

function StatsStrip({
  ready,
  pending,
  errored,
  polling,
  onRefresh,
}: {
  ready: number;
  pending: number;
  errored: number;
  polling: boolean;
  onRefresh: () => void;
}) {
  const Stat = ({ n, label }: { n: number; label: string }) => (
    <span className="inline-flex items-baseline gap-1.5">
      <span className="text-[14px] font-semibold tabular-nums text-foreground">
        {n}
      </span>
      <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
        {label}
      </span>
    </span>
  );
  return (
    <div className="flex items-center justify-between gap-4 border-y border-border/50 py-2">
      <div className="flex items-baseline gap-5">
        <Stat n={ready} label="ready" />
        <span aria-hidden className="text-muted-foreground/40">·</span>
        <Stat n={pending} label="pending" />
        <span aria-hidden className="text-muted-foreground/40">·</span>
        <Stat n={errored} label="errored" />
      </div>
      <div className="flex items-center gap-3">
        {polling && (
          <span className="text-[10px] font-mono text-muted-foreground">
            polling…
          </span>
        )}
        <button
          onClick={onRefresh}
          className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
        >
          refresh
        </button>
      </div>
    </div>
  );
}

function ResumeChip({
  row,
  onOpen,
}: {
  row: AnalysisRow;
  onOpen: () => void;
}) {
  return (
    <button
      onClick={onOpen}
      className="w-full flex items-center gap-3 rounded-md border border-border/60 bg-muted/20 hover:bg-muted/40 px-3.5 py-2.5 transition-colors text-left"
    >
      <span aria-hidden className="text-[14px] leading-none">↻</span>
      <div className="flex-1 min-w-0">
        <div className="flex items-baseline gap-2">
          <span className="text-[11px] font-mono text-muted-foreground uppercase tracking-wide">
            Resume
          </span>
          <span className="text-[12px] font-mono text-foreground truncate">
            {prettyRowLabel(row)}
          </span>
        </div>
        <p className="text-[10px] font-mono text-muted-foreground mt-0.5">
          last touched {formatRelativeTime(row.updated_at)}
        </p>
      </div>
      <span className="text-[12px] text-muted-foreground" aria-hidden>
        →
      </span>
    </button>
  );
}

function Feed({
  history,
  onOpen,
  onDismiss,
  onRetry,
}: {
  history: AnalysisRow[];
  onOpen: (row: AnalysisRow) => void;
  onDismiss: (row: AnalysisRow) => void;
  onRetry: (row: AnalysisRow) => void;
}) {
  const [showErrored, setShowErrored] = useState(false);
  const [selected, setSelected] = useState<string[]>([]);
  const [pair, setPair] = useState<[AnalysisRow, AnalysisRow] | null>(null);
  const errored = history.filter((r) => r.status === "errored");
  const visible = showErrored
    ? history
    : history.filter((r) => r.status !== "errored");
  const toggleSelect = (id: string) => {
    setSelected((prev) => {
      if (prev.includes(id)) return prev.filter((x) => x !== id);
      if (prev.length >= 2) return [prev[1], id];
      return [...prev, id];
    });
  };
  const openCompare = () => {
    if (selected.length !== 2) return;
    const a = history.find((r) => r.id === selected[0]);
    const b = history.find((r) => r.id === selected[1]);
    if (a && b) setPair([a, b]);
  };

  return (
    <section className="space-y-2">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Recent PRs
        </h2>
        {selected.length === 2 ? (
          <button
            onClick={openCompare}
            className="text-[10px] font-mono rounded border border-foreground/60 bg-foreground/90 text-background px-2 py-0.5 hover:bg-foreground"
          >
            ⇄ compare 2
          </button>
        ) : selected.length > 0 ? (
          <span className="text-[10px] font-mono text-muted-foreground">
            {selected.length}/2 picked for compare
          </span>
        ) : null}
      </div>
      {visible.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">
          {showErrored || errored.length === 0
            ? "Nothing yet. Start an analysis — results persist across restarts."
            : "No ready runs yet."}
        </p>
      ) : (
        <ol className="space-y-2">
          {visible.map((r) => (
            <li key={r.id}>
              <FeedCard
                row={r}
                selected={selected.includes(r.id)}
                onToggleSelect={() => toggleSelect(r.id)}
                onOpen={() => onOpen(r)}
                onDismiss={() => onDismiss(r)}
                onRetry={() => onRetry(r)}
              />
            </li>
          ))}
        </ol>
      )}
      {errored.length > 0 && (
        <button
          onClick={() => setShowErrored((x) => !x)}
          className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
        >
          {showErrored
            ? `hide ${errored.length} errored`
            : `show ${errored.length} errored`}
        </button>
      )}
      {pair && (
        <CompareView
          a={pair[0]}
          b={pair[1]}
          onClose={() => {
            setPair(null);
            setSelected([]);
          }}
        />
      )}
    </section>
  );
}

/**
 * One feed card. Status dot on the left, repo label + sha + time on
 * the main line, actions on hover. Clicking the card body opens the
 * PR (when ready). Compare-select checkbox surfaces on hover too.
 */
function FeedCard({
  row,
  selected,
  onToggleSelect,
  onOpen,
  onDismiss,
  onRetry,
}: {
  row: AnalysisRow;
  selected: boolean;
  onToggleSelect: () => void;
  onOpen: () => void;
  onDismiss: () => void;
  onRetry: () => void;
}) {
  const canOpen = row.status === "ready";
  const canRetry = row.status === "errored";
  return (
    <article
      className={
        "group rounded-lg border transition-colors " +
        (selected
          ? "border-foreground/60 bg-muted/40"
          : "border-border/60 bg-background hover:border-border hover:bg-muted/20")
      }
    >
      <div className="flex items-center gap-3 px-3.5 py-3">
        <StatusDot status={row.status} />
        <button
          onClick={onOpen}
          disabled={!canOpen}
          className="flex-1 min-w-0 text-left disabled:cursor-not-allowed"
        >
          <div className="flex items-baseline gap-2">
            <span className="text-[13px] font-mono font-medium text-foreground truncate">
              {prettyRowLabel(row)}
            </span>
            <StatusChip status={row.status} />
          </div>
          <div className="flex items-baseline gap-3 mt-0.5 text-[10px] font-mono text-muted-foreground">
            <span title={row.head_sha}>{row.head_sha.slice(0, 8)}</span>
            <span>{formatRelativeTime(row.updated_at)}</span>
            {row.status === "errored" && row.message && (
              <span className="truncate text-muted-foreground/80">
                · {row.message}
              </span>
            )}
          </div>
        </button>
        <div className="flex items-center gap-1 shrink-0 opacity-0 group-hover:opacity-100 focus-within:opacity-100 transition-opacity">
          {canOpen && (
            <label
              className="inline-flex items-center gap-1 text-[10px] font-mono text-muted-foreground hover:text-foreground px-1.5 py-0.5 rounded hover:bg-muted/50 cursor-pointer"
              title="Pick two ready rows to compare"
            >
              <input
                type="checkbox"
                checked={selected}
                onChange={onToggleSelect}
                aria-label={`pick ${prettyRowLabel(row)} for compare`}
              />
              <span>compare</span>
            </label>
          )}
          {canRetry && (
            <button
              onClick={onRetry}
              className="text-[10px] font-mono text-muted-foreground hover:text-foreground px-1.5 py-0.5 rounded hover:bg-muted/50"
            >
              retry
            </button>
          )}
          <button
            onClick={onDismiss}
            className="text-[10px] font-mono text-muted-foreground hover:text-destructive px-1.5 py-0.5 rounded hover:bg-muted/50"
            title="Archive — removes from the list; cached artifact stays."
          >
            archive
          </button>
        </div>
      </div>
    </article>
  );
}

function StatusDot({ status }: { status: AnalysisRow["status"] }) {
  const cls =
    status === "ready"
      ? "bg-emerald-500/70"
      : status === "pending"
        ? "bg-amber-400/80 animate-pulse"
        : "bg-rose-500/70";
  return (
    <span
      aria-hidden
      className={"inline-block w-2 h-2 rounded-full shrink-0 " + cls}
    />
  );
}

function StatusChip({ status }: { status: AnalysisRow["status"] }) {
  if (status === "pending") {
    return (
      <span className="inline-flex items-center gap-1 text-[9px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-border/60 bg-background/60 text-muted-foreground">
        <PulseDot />
        <span>running</span>
      </span>
    );
  }
  if (status === "errored") {
    return (
      <span className="text-[9px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-destructive/40 bg-destructive/10 text-destructive">
        error
      </span>
    );
  }
  return (
    <span className="text-[9px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-border/60 bg-background/60 text-muted-foreground">
      ready
    </span>
  );
}

function PulseDot() {
  return (
    <span className="relative inline-flex" aria-hidden>
      <span className="absolute inset-0 w-1 h-1 rounded-full bg-muted-foreground/40 animate-ping" />
      <span className="w-1 h-1 rounded-full bg-muted-foreground/80" />
    </span>
  );
}

function AnalyseCard({
  base,
  head,
  prUrl,
  busy,
  onBase,
  onHead,
  onPrUrl,
  onAnalyse,
  onAnalyseUrl,
  intentLabel,
  intentErr,
  onIntentFile,
}: {
  base: string;
  head: string;
  prUrl: string;
  busy: boolean;
  onBase: (v: string) => void;
  onHead: (v: string) => void;
  onPrUrl: (v: string) => void;
  onAnalyse: () => void;
  onAnalyseUrl: () => void;
  intentLabel: string | null;
  intentErr: string | null;
  onIntentFile: (f: File | null) => void;
}) {
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const urlValid = /^https:\/\/(www\.)?github\.com\/[^/]+\/[^/]+\/pull\/\d+/.test(
    prUrl.trim(),
  );

  return (
    <section className="rounded-lg border border-border/60 bg-muted/10 overflow-hidden max-w-3xl">
      <header className="px-4 pt-2.5 pb-2 border-b border-border/60 flex items-baseline justify-between">
        <h2 className="text-[12px] font-semibold text-foreground">
          Analyse a PR
        </h2>
        <p className="text-[10px] font-mono text-muted-foreground uppercase tracking-wide">
          intent · proof · cost
        </p>
      </header>

      <div className="p-3 space-y-3">
        <div className="space-y-1">
          <div className="flex gap-2">
            <input
              id="pr-url"
              value={prUrl}
              onChange={(e) => onPrUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && urlValid && !busy) onAnalyseUrl();
              }}
              placeholder="https://github.com/owner/repo/pull/123"
              className="flex-1 text-[12px] font-mono border rounded px-2 py-1.5 bg-background placeholder:text-muted-foreground/60 focus:outline-none focus:ring-1 focus:ring-ring"
            />
            <button
              onClick={onAnalyseUrl}
              disabled={busy || !urlValid}
              className="text-[12px] font-medium rounded border border-foreground/80 bg-foreground text-background px-3 py-1.5 hover:bg-foreground/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              {busy ? "Analysing…" : "Analyse"}
            </button>
          </div>
          <p className="text-[10px] text-muted-foreground leading-snug">
            Any public GitHub PR URL — body becomes intent; base + head clone at resolved SHAs.
          </p>
        </div>

        <IntentDropZone
          label={intentLabel}
          error={intentErr}
          onFile={onIntentFile}
        />

        <details
          open={advancedOpen}
          onToggle={(e) =>
            setAdvancedOpen((e.target as HTMLDetailsElement).open)
          }
          className="rounded-md border border-border/50 bg-background/40"
        >
          <summary className="cursor-pointer select-none px-4 py-2.5 text-[12px] font-medium text-muted-foreground hover:text-foreground transition-colors flex items-baseline gap-2">
            <span aria-hidden className="text-muted-foreground">
              {advancedOpen ? "▾" : "▸"}
            </span>
            <span>Local paths</span>
            <span className="text-[10px] font-mono text-muted-foreground">
              · advanced · works today
            </span>
          </summary>
          <div className="px-4 pt-1 pb-4 space-y-3 border-t border-border/40">
            <p className="text-[11px] text-muted-foreground leading-snug">
              Point at two absolute paths — usually{" "}
              <code className="text-[10px]">git worktree</code> snapshots of
              the PR&apos;s base and head. Runs locally; nothing leaves your
              machine unless a remote LLM backend is configured for the
              intent/proof passes.
            </p>
            <div className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-2 items-center">
              <label
                htmlFor="base"
                className="text-[11px] font-mono text-muted-foreground"
              >
                base
              </label>
              <input
                id="base"
                className="text-[12px] font-mono border rounded px-2 py-1.5 bg-background focus:outline-none focus:ring-1 focus:ring-ring"
                placeholder="/absolute/path/to/pr/base"
                value={base}
                onChange={(e) => onBase(e.target.value)}
              />
              <label
                htmlFor="head"
                className="text-[11px] font-mono text-muted-foreground"
              >
                head
              </label>
              <input
                id="head"
                className="text-[12px] font-mono border rounded px-2 py-1.5 bg-background focus:outline-none focus:ring-1 focus:ring-ring"
                placeholder="/absolute/path/to/pr/head"
                value={head}
                onChange={(e) => onHead(e.target.value)}
              />
            </div>
            <div className="flex justify-end pt-1">
              <button
                onClick={onAnalyse}
                disabled={busy || !base || !head}
                className="text-[12px] font-medium rounded-md border border-foreground/70 bg-foreground/90 text-background px-4 py-1.5 hover:bg-foreground disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
              >
                {busy ? "Analysing…" : "Analyse (local)"}
              </button>
            </div>
          </div>
        </details>
      </div>
    </section>
  );
}

/** Translate raw backend errors into short, human copy. Keeps the
 *  original as a collapsible "technical detail" so we never hide the
 *  ground truth — just soften the first read. */
function ErrorCard({ raw, onDismiss }: { raw: string; onDismiss: () => void }) {
  const { title, hint } = interpretError(raw);
  return (
    <section className="rounded-md border border-rose-400/50 bg-rose-50 dark:bg-rose-400/[0.07] px-3.5 py-2.5 space-y-1.5">
      <div className="flex items-baseline justify-between gap-3">
        <p className="text-[12px] font-semibold text-rose-800 dark:text-rose-200">
          {title}
        </p>
        <button
          onClick={onDismiss}
          className="text-[10px] font-mono text-rose-700/70 dark:text-rose-300/70 hover:text-rose-900 dark:hover:text-rose-100"
        >
          dismiss
        </button>
      </div>
      {hint && (
        <p className="text-[11px] text-rose-700 dark:text-rose-300/90 leading-snug">
          {hint}
        </p>
      )}
      <details className="text-[10px] font-mono text-rose-700/70 dark:text-rose-300/70">
        <summary className="cursor-pointer select-none hover:text-rose-900 dark:hover:text-rose-100">
          technical detail
        </summary>
        <pre className="mt-1 whitespace-pre-wrap break-words">{raw}</pre>
      </details>
    </section>
  );
}

function interpretError(raw: string): { title: string; hint?: string } {
  const s = raw.toLowerCase();
  if (s.includes("401") || s.includes("sign in")) {
    return {
      title: "Not signed in",
      hint: "Sign in with GitHub so we can read the PR and clone its refs.",
    };
  }
  if (s.includes("403")) {
    return {
      title: "GitHub denied access",
      hint: "The stored token may be expired. Sign out + back in, then retry.",
    };
  }
  if (s.includes("404") || s.includes("not a pr url") || s.includes("github PR fetch 404")) {
    return {
      title: "PR not found",
      hint: "Double-check the URL — this worked in my browser: https://github.com/owner/repo/pull/123.",
    };
  }
  if (s.includes("rate") && s.includes("limit")) {
    return {
      title: "GitHub rate-limited us",
      hint: "Wait a minute and retry — the client-hour budget is shared across signed-in sessions.",
    };
  }
  if (s.includes("git ") && (s.includes("fatal") || s.includes("failed"))) {
    return {
      title: "Clone failed",
      hint: "The repo or SHA isn't reachable with the current scopes. Public repos only for now.",
    };
  }
  if (s.includes("invalid url") || s.includes("not a github") || s.includes("not a pr url")) {
    return {
      title: "Invalid PR URL",
      hint: "Expected `https://github.com/<owner>/<repo>/pull/<n>`.",
    };
  }
  if (s.includes("networkerror") || s.includes("failed to fetch")) {
    return {
      title: "Can't reach the backend",
      hint: "The floe-server isn't responding on :8787. Check it's running.",
    };
  }
  return { title: "Something went wrong" };
}

/** Sidebar row label. URL-driven runs land as `owner/repo #N`; legacy
 *  local-path runs landed as the head dir's leaf name (e.g. `head`,
 *  `glide-mq-head-181`) which leaks paths into the UI. Anything that
 *  isn't a clean `owner/repo` form falls back to `local · <sha>`. */
function prettyRowLabel(row: AnalysisRow): string {
  const repo = row.repo?.trim() ?? "";
  const sha8 = row.head_sha.slice(0, 8);
  if (!repo) return `local · ${sha8}`;
  // Looks like a real GitHub identity: `owner/repo` optionally with `#N`.
  if (/^[\w.-]+\/[\w.-]+(\s+#\d+)?$/.test(repo)) return repo;
  // Anything else (path leaks like `head` or `glide-mq-head-181`)
  // gets the neutral fallback so the sidebar doesn't look broken.
  return `local · ${sha8}`;
}

function formatRelativeTime(iso: string): string {
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return iso;
  const seconds = Math.max(0, Math.round((Date.now() - then) / 1000));
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.round(seconds / 60);
  if (minutes < 60) return `${minutes}m ago`;
  const hours = Math.round(minutes / 60);
  if (hours < 48) return `${hours}h ago`;
  const days = Math.round(hours / 24);
  return `${days}d ago`;
}

function IntentDropZone({
  label,
  error,
  onFile,
}: {
  label: string | null;
  error: string | null;
  onFile: (f: File | null) => void;
}) {
  const [hover, setHover] = useState(false);
  return (
    <div
      onDragOver={(e) => {
        e.preventDefault();
        setHover(true);
      }}
      onDragLeave={() => setHover(false)}
      onDrop={(e) => {
        e.preventDefault();
        setHover(false);
        const f = e.dataTransfer.files?.[0];
        if (f) onFile(f);
      }}
      className={
        "rounded-md border border-dashed px-4 py-3 transition-colors " +
        (hover ? "border-foreground/60 bg-muted/30" : "border-border/60 bg-background/40")
      }
    >
      <div className="flex items-baseline gap-3">
        <label className="text-[12px] font-medium text-foreground cursor-pointer">
          <input
            type="file"
            accept="application/json,.json"
            className="hidden"
            onChange={(e) => onFile(e.target.files?.[0] ?? null)}
          />
          <span className="underline underline-offset-2 decoration-dotted">
            Choose intent.json
          </span>
        </label>
        <span className="text-[11px] text-muted-foreground">or drop a file here</span>
        {label && (
          <span className="ml-auto text-[11px] font-mono text-foreground">
            · {label}
            <button
              type="button"
              onClick={() => onFile(null)}
              className="ml-2 text-muted-foreground hover:text-destructive"
              aria-label="clear intent file"
            >
              ×
            </button>
          </span>
        )}
      </div>
      <p className="text-[11px] text-muted-foreground mt-1 leading-snug">
        Optional — structured claims or raw text. Falls back to the PR body
        when analysing from a GitHub URL.
      </p>
      {error && (
        <p className="text-[11px] text-destructive font-mono mt-1">{error}</p>
      )}
    </div>
  );
}
