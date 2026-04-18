import { useState } from "react";
import { analyze, pollUntilDone, type JobView } from "@/api";
import type { LoadedJob } from "@/App";

interface Props {
  onJob: (j: LoadedJob | null) => void;
}

/** Landing form — base/head paths + analyze button. Shown when no PR is
 *  loaded. The palette will eventually provide a faster path to reloading
 *  a recent PR, but for v0 this is the entry point. */
export function LoadForm({ onJob }: Props) {
  const [base, setBase] = useState(localStorage.getItem("adr.base") ?? "");
  const [head, setHead] = useState(localStorage.getItem("adr.head") ?? "");
  const [job, setJob] = useState<JobView | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function run() {
    setBusy(true);
    setErr(null);
    setJob({ status: "pending" });
    localStorage.setItem("adr.base", base);
    localStorage.setItem("adr.head", head);
    try {
      const id = await analyze(base, head);
      const done = await pollUntilDone(id);
      setJob(done);
      if (done.artifact) {
        onJob({ jobId: id, artifact: done.artifact });
      } else {
        onJob(null);
      }
    } catch (e) {
      setErr(String(e));
      setJob(null);
      onJob(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-5 max-w-3xl">
      <h1 className="text-[15px] font-semibold text-foreground">Load a PR</h1>
      <section className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-2 items-center">
        <label className="text-[12px] text-muted-foreground">Base</label>
        <input
          className="text-[12px] font-mono border rounded px-2 py-1 bg-background focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder="/absolute/path/to/pr/base"
          value={base}
          onChange={(e) => setBase(e.target.value)}
        />
        <label className="text-[12px] text-muted-foreground">Head</label>
        <input
          className="text-[12px] font-mono border rounded px-2 py-1 bg-background focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder="/absolute/path/to/pr/head"
          value={head}
          onChange={(e) => setHead(e.target.value)}
        />
        <div />
        <div>
          <button
            onClick={run}
            disabled={busy || !base || !head}
            className="text-[12px] font-medium border rounded px-3 py-1 hover:bg-muted disabled:opacity-50 transition-colors"
          >
            {busy ? "Analyzing…" : "Analyze"}
          </button>
        </div>
      </section>

      {err && (
        <section className="text-[12px] font-mono text-destructive border border-destructive/40 rounded px-3 py-2">
          {err}
        </section>
      )}

      {job?.status === "pending" && (
        <section className="text-[12px] text-muted-foreground">Analyzing…</section>
      )}
    </div>
  );
}
