import { useState } from "react";
import { analyze, pollUntilDone, type JobView } from "@/api";
import type { Artifact } from "@/types/artifact";

interface Props {
  artifact: Artifact | null;
  onArtifact: (a: Artifact | null) => void;
}

/**
 * Temporary PR entry screen. No design opinions yet beyond the typography
 * rules — base/head path inputs, one button, a raw JSON dump below when a
 * job completes. We replace the JSON dump with real PR overview once we
 * settle on the pr-view layout.
 */
export function PrView({ artifact, onArtifact }: Props) {
  const [base, setBase] = useState(localStorage.getItem("adr.base") ?? "");
  const [head, setHead] = useState(localStorage.getItem("adr.head") ?? "");
  const [job, setJob] = useState<JobView | null>(
    artifact ? { status: "ready", artifact } : null,
  );
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
      onArtifact(done.artifact ?? null);
    } catch (e) {
      setErr(String(e));
      setJob(null);
      onArtifact(null);
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-5">
      <section className="grid grid-cols-[auto,1fr] gap-x-3 gap-y-2 items-center max-w-3xl">
        <label className="text-[12px] font-mono text-muted-foreground">base</label>
        <input
          className="text-[12px] font-mono border rounded px-2 py-1 bg-background focus:outline-none focus:ring-1 focus:ring-ring"
          placeholder="/absolute/path/to/pr/base"
          value={base}
          onChange={(e) => setBase(e.target.value)}
        />
        <label className="text-[12px] font-mono text-muted-foreground">head</label>
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
            {busy ? "analyzing…" : "analyze"}
          </button>
        </div>
      </section>

      {err && (
        <section className="text-[12px] font-mono text-destructive border border-destructive/40 rounded px-3 py-2">
          {err}
        </section>
      )}

      {job && (
        <section className="space-y-2">
          <div className="text-[12px] font-mono text-muted-foreground">
            status: <span className="text-foreground">{job.status}</span>
            {job.message && <> · {job.message}</>}
          </div>
          {job.artifact && (
            <pre className="text-[12px] font-mono bg-muted/60 rounded p-3 overflow-x-auto max-h-[70vh]">
              {JSON.stringify(job.artifact, null, 2)}
            </pre>
          )}
        </section>
      )}
    </div>
  );
}
