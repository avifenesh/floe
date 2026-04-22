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
  const [base, setBase] = useState(localStorage.getItem("adr.base") ?? "");
  const [head, setHead] = useState(localStorage.getItem("adr.head") ?? "");
  const [prUrl, setPrUrl] = useState("");
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
    localStorage.setItem("adr.base", base);
    localStorage.setItem("adr.head", head);
    try {
      const id = await analyze(base, head);
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

  return (
    <div className="max-w-6xl mx-auto space-y-5">
      <UserBar me={me} onSignOut={onSignOut} />

      <div className="grid grid-cols-1 lg:grid-cols-[340px,1fr] gap-6">
        <Sidebar
          history={history}
          hasPending={hasPending}
          onRefresh={() => void refreshHistory()}
          onOpen={(r) => void openHistory(r)}
          onDismiss={(r) => void dismiss(r)}
          onRetry={(r) => void retry(r)}
        />

        <div className="space-y-5 min-w-0">
          {pendingJobId ? (
            <PipelineProgress
              jobId={pendingJobId}
              backendBase={PIPELINE_BACKEND}
            />
          ) : (
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
            />
          )}
          {err && <ErrorCard raw={err} onDismiss={() => setErr(null)} />}
        </div>
      </div>
    </div>
  );
}

function UserBar({ me, onSignOut }: { me: Me; onSignOut: () => void }) {
  return (
    <header className="relative flex items-start justify-between gap-3">
      <div>
        <h1 className="text-[18px] font-semibold text-foreground leading-tight">
          Welcome back{me.display_name ? `, ${firstName(me.display_name)}` : ""}.
        </h1>
        <p className="text-[12px] text-muted-foreground">
          Pick up a recent PR from the sidebar or start a new analysis.
        </p>
      </div>
      <button
        onClick={onSignOut}
        title="Sign out"
        className="group flex flex-col items-center gap-1 text-center"
      >
        {me.avatar_url && (
          <img
            src={me.avatar_url}
            alt=""
            className="w-9 h-9 rounded-full border border-border/60 group-hover:opacity-80 transition-opacity"
          />
        )}
        <span className="text-[12px] font-mono text-foreground">
          {me.display_name ?? me.provider_user_id}
        </span>
        <span className="text-[10px] font-mono text-muted-foreground/70 group-hover:text-muted-foreground transition-colors">
          sign out
        </span>
      </button>
    </header>
  );
}

function firstName(full: string): string {
  const trimmed = full.trim();
  return trimmed.split(/\s+/)[0] ?? trimmed;
}

function Sidebar({
  history,
  hasPending,
  onRefresh,
  onOpen,
  onDismiss,
  onRetry,
}: {
  history: AnalysisRow[];
  hasPending: boolean;
  onRefresh: () => void;
  onOpen: (row: AnalysisRow) => void;
  onDismiss: (row: AnalysisRow) => void;
  onRetry: (row: AnalysisRow) => void;
}) {
  return (
    <aside className="space-y-3">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Recent PRs
        </h2>
        <div className="flex items-baseline gap-2">
          {hasPending && (
            <span
              className="text-[10px] font-mono text-muted-foreground"
              title="Auto-refreshes every 5s while any run is pending"
            >
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
      {history.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">
          Nothing yet. Start an analysis — results persist across restarts.
        </p>
      ) : (
        <ol className="space-y-1.5">
          {history.map((r) => (
            <li key={r.id}>
              <HistoryRow
                row={r}
                onOpen={() => onOpen(r)}
                onDismiss={() => onDismiss(r)}
                onRetry={() => onRetry(r)}
              />
            </li>
          ))}
        </ol>
      )}
    </aside>
  );
}

function HistoryRow({
  row,
  onOpen,
  onDismiss,
  onRetry,
}: {
  row: AnalysisRow;
  onOpen: () => void;
  onDismiss: () => void;
  onRetry: () => void;
}) {
  const canOpen = row.status === "ready";
  const canRetry = row.status === "errored";
  return (
    <div className="group rounded border border-border/60 bg-muted/10 hover:bg-muted/30 transition-colors overflow-hidden">
      <button
        onClick={onOpen}
        disabled={!canOpen}
        className="w-full text-left px-3 pt-2 pb-1.5 disabled:cursor-not-allowed"
      >
        <div className="flex items-baseline justify-between gap-2">
          <span className="text-[12px] font-mono text-foreground truncate">
            {prettyRowLabel(row)}
          </span>
          <StatusChip status={row.status} />
        </div>
        <div className="flex items-baseline gap-2 mt-0.5 text-[10px] font-mono text-muted-foreground">
          <span title={row.head_sha}>{row.head_sha.slice(0, 8)}</span>
          <span className="ml-auto">{formatRelativeTime(row.updated_at)}</span>
        </div>
        {row.message && row.status === "errored" && (
          <p className="text-[10px] text-muted-foreground mt-1 line-clamp-2 text-left">
            {row.message}
          </p>
        )}
      </button>
      <div className="flex items-center gap-1 px-2 pb-2 opacity-0 group-hover:opacity-100 transition-opacity">
        {canOpen && (
          <button
            onClick={onOpen}
            className="text-[10px] font-mono text-muted-foreground hover:text-foreground px-1.5 py-0.5 rounded hover:bg-muted/50"
          >
            open
          </button>
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
          className="ml-auto text-[10px] font-mono text-muted-foreground hover:text-destructive px-1.5 py-0.5 rounded hover:bg-muted/50"
        >
          dismiss
        </button>
      </div>
    </div>
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
}) {
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const urlValid = /^https:\/\/(www\.)?github\.com\/[^/]+\/[^/]+\/pull\/\d+/.test(
    prUrl.trim(),
  );

  return (
    <section className="rounded-xl border border-border/60 bg-muted/10 overflow-hidden">
      <header className="px-5 pt-4 pb-3 border-b border-border/60 flex items-baseline justify-between">
        <h2 className="text-[14px] font-semibold text-foreground">
          Analyse a PR
        </h2>
        <p className="text-[10px] font-mono text-muted-foreground uppercase tracking-wide">
          intent + proof + cost
        </p>
      </header>

      <div className="p-5 space-y-4">
        <div className="space-y-1.5">
          <label
            htmlFor="pr-url"
            className="text-[11px] font-medium text-foreground"
          >
            GitHub PR URL
          </label>
          <div className="flex gap-2">
            <input
              id="pr-url"
              value={prUrl}
              onChange={(e) => onPrUrl(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && urlValid && !busy) onAnalyseUrl();
              }}
              placeholder="https://github.com/owner/repo/pull/123"
              className="flex-1 text-[13px] font-mono border rounded-md px-3 py-2 bg-background placeholder:text-muted-foreground/60 focus:outline-none focus:ring-1 focus:ring-ring"
            />
            <button
              onClick={onAnalyseUrl}
              disabled={busy || !urlValid}
              className="text-[13px] font-medium rounded-md border border-foreground/80 bg-foreground text-background px-4 py-2 hover:bg-foreground/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors"
            >
              {busy ? "Analysing…" : "Analyse"}
            </button>
          </div>
          <p className="text-[11px] text-muted-foreground leading-snug">
            Paste any public GitHub PR URL — the server pulls the PR
            body as intent and clones base + head at the resolved SHAs.
          </p>
        </div>

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
      hint: "The adr-server isn't responding on :8787. Check it's running.",
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
