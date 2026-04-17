import { useState } from "react";
import { analyze, pollUntilDone, type JobView } from "./api";

/**
 * Scaffold shell. No visual opinions yet. Two inputs for the base/head paths,
 * one button to analyze, a status line, and the raw artifact JSON dumped below.
 * Every piece of this page is placeholder — we replace each view together.
 */
export default function App() {
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
    } catch (e) {
      setErr(String(e));
      setJob(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="min-h-screen">
      <header className="border-b">
        <div className="mx-auto max-w-5xl px-6 py-4">
          <h1 className="text-sm font-mono text-muted-foreground">adr · scaffold</h1>
        </div>
      </header>

      <main className="mx-auto max-w-5xl px-6 py-6 space-y-4">
        <section className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-2 text-sm items-center">
          <label className="font-mono text-muted-foreground">base</label>
          <input
            className="font-mono text-xs border rounded px-2 py-1 bg-background"
            placeholder="/absolute/path/to/pr/base"
            value={base}
            onChange={(e) => setBase(e.target.value)}
          />
          <label className="font-mono text-muted-foreground">head</label>
          <input
            className="font-mono text-xs border rounded px-2 py-1 bg-background"
            placeholder="/absolute/path/to/pr/head"
            value={head}
            onChange={(e) => setHead(e.target.value)}
          />
          <div />
          <div>
            <button
              onClick={run}
              disabled={busy || !base || !head}
              className="text-xs font-medium border rounded px-3 py-1 hover:bg-muted disabled:opacity-50"
            >
              {busy ? "analyzing…" : "analyze"}
            </button>
          </div>
        </section>

        {err && (
          <section className="text-xs font-mono text-destructive border border-destructive/40 rounded px-3 py-2">
            {err}
          </section>
        )}

        {job && (
          <section className="space-y-2">
            <div className="text-xs font-mono text-muted-foreground">
              status: <span className="text-foreground">{job.status}</span>
              {job.message && <> · {job.message}</>}
            </div>
            {job.artifact && (
              <pre className="text-xs font-mono bg-muted rounded p-3 overflow-x-auto max-h-[70vh]">
                {JSON.stringify(job.artifact, null, 2)}
              </pre>
            )}
          </section>
        )}
      </main>
    </div>
  );
}
