import { useEffect, useMemo, useState } from "react";
import { fetchFile } from "@/api";
import { changedFiles } from "@/lib/artifact";
import { clipContext, lineDiff } from "@/lib/diff";
import type { Artifact } from "@/types/artifact";
import { DiffView } from "./source/DiffView";
import { FileList } from "./source/FileList";

interface Props {
  artifact: Artifact;
  jobId: string;
}

/**
 * Source view · pass 1. Shows the list of files on the left, the selected
 * file's unified diff on the right. No syntax highlighting yet — we add
 * shiki next. Word-level highlights land in the pass after that.
 */
export function SourceView({ artifact, jobId }: Props) {
  const files = useMemo(() => changedFiles(artifact), [artifact]);
  const visible = files.filter((f) => f.status !== "unchanged");
  const [selected, setSelected] = useState<string | null>(
    visible[0]?.path ?? files[0]?.path ?? null,
  );

  return (
    <div className="grid grid-cols-[220px,1fr] gap-6">
      <aside>
        <h2 className="text-[11px] font-medium text-muted-foreground mb-2 tracking-wide">
          Files
        </h2>
        <FileList files={visible.length > 0 ? visible : files} selected={selected} onSelect={setSelected} />
      </aside>
      <section>
        {selected ? (
          <FileDiff jobId={jobId} path={selected} status={files.find((f) => f.path === selected)?.status ?? "unchanged"} />
        ) : (
          <div className="text-[12px] text-muted-foreground">Select a file.</div>
        )}
      </section>
    </div>
  );
}

function FileDiff({
  jobId,
  path,
  status,
}: {
  jobId: string;
  path: string;
  status: "added" | "removed" | "modified" | "unchanged";
}) {
  const [base, setBase] = useState<string | null>(null);
  const [head, setHead] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let abandoned = false;
    setBase(null);
    setHead(null);
    setErr(null);
    const load = async () => {
      try {
        const [b, h] = await Promise.all([
          status === "added" ? Promise.resolve("") : fetchFile(jobId, "base", path),
          status === "removed" ? Promise.resolve("") : fetchFile(jobId, "head", path),
        ]);
        if (abandoned) return;
        setBase(b);
        setHead(h);
      } catch (e) {
        if (!abandoned) setErr(String(e));
      }
    };
    load();
    return () => {
      abandoned = true;
    };
  }, [jobId, path, status]);

  if (err) {
    return (
      <div className="text-[12px] font-mono text-destructive border border-destructive/40 rounded px-3 py-2">
        {err}
      </div>
    );
  }
  if (base === null || head === null) {
    return <div className="text-[12px] text-muted-foreground">Loading…</div>;
  }
  const rows = clipContext(lineDiff(base, head), 3);
  return (
    <div className="space-y-2">
      <div className="text-[12px] font-mono text-muted-foreground">
        <span className="text-foreground">{path}</span>
        <span className="mx-2">·</span>
        <span>{status}</span>
      </div>
      <DiffView rows={rows} />
    </div>
  );
}
