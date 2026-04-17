import { useEffect, useMemo, useState } from "react";
import { fetchFile } from "@/api";
import { changedFiles } from "@/lib/artifact";
import { clipContext, enrichWordLevel, lineDiff } from "@/lib/diff";
import type { Artifact } from "@/types/artifact";
import { DiffView } from "./source/DiffView";
import { FileTabs } from "./source/FileTabs";

interface Props {
  artifact: Artifact;
  jobId: string;
}

/**
 * Source view. IDE-style tab bar up top, one tab per changed file; the
 * active tab's unified diff renders below. No sidebar — the file list
 * lives in the tab row, keeping the full page width for the code.
 */
export function SourceView({ artifact, jobId }: Props) {
  const files = useMemo(() => changedFiles(artifact), [artifact]);
  const visible = files.filter((f) => f.status !== "unchanged");
  const list = visible.length > 0 ? visible : files;
  const [selected, setSelected] = useState<string | null>(list[0]?.path ?? null);

  return (
    <div className="flex flex-col gap-3">
      <FileTabs files={list} selected={selected} onSelect={setSelected} />
      <div>
        {selected ? (
          <FileDiff
            jobId={jobId}
            path={selected}
            status={files.find((f) => f.path === selected)?.status ?? "unchanged"}
          />
        ) : (
          <div className="text-[12px] text-muted-foreground">No files.</div>
        )}
      </div>
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
  const entries = clipContext(enrichWordLevel(lineDiff(base, head)), 3);
  return <DiffView entries={entries} />;
}
