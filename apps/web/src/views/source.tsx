import { useEffect, useMemo, useState } from "react";
import { fetchFile } from "@/api";
import {
  byteToLine,
  changedFiles,
  flowsByFile as flowsByFileFn,
  hunkCountByFile,
  hunkTouches,
  type HunkClass,
} from "@/lib/artifact";
import { clipContext, enrichWordLevel, lineDiff } from "@/lib/diff";
import { highlight, langForPath, type HighlightedLines } from "@/lib/highlight";
import { useTheme } from "@/lib/theme";

import type { Artifact } from "@/types/artifact";
import { DiffView, type LineTouches } from "./source/DiffView";
import { FileSidebar } from "./source/FileSidebar";

interface Props {
  artifact: Artifact;
  jobId: string;
  /** When set, restrict the sidebar to this file set. Used by the
   *  Flow workspace to scope Source to files the flow touches. */
  scope?: { files: Set<string> };
}

/**
 * Source view. File sidebar on the left; the active file's unified diff
 * renders on the right. Optionally scoped to a subset of files (flow
 * workspace) — when scoped, the sidebar heading shifts from "N files" to
 * "N of M files".
 */
export function SourceView({ artifact, jobId, scope }: Props) {
  const files = useMemo(() => changedFiles(artifact), [artifact]);
  const visible = files.filter((f) => f.status !== "unchanged");
  const base = visible.length > 0 ? visible : files;
  const list = scope
    ? base.filter((f) => scope.files.has(f.path))
    : base;
  const [selected, setSelected] = useState<string | null>(list[0]?.path ?? null);

  // If the flow selection changes and the previously-selected file isn't in
  // the new scope, drop the selection so the user lands on a valid file.
  useEffect(() => {
    if (selected && !list.some((f) => f.path === selected)) {
      setSelected(list[0]?.path ?? null);
    }
  }, [list, selected]);

  const counts = useMemo(() => hunkCountByFile(artifact), [artifact]);
  const byFile = useMemo(() => flowsByFileFn(artifact), [artifact]);

  return (
    <div className="grid grid-cols-[clamp(180px,22%,280px)_1fr] gap-4 items-start">
      <FileSidebar
        files={list}
        selected={selected}
        onSelect={setSelected}
        hunkCounts={counts}
        flowsByFile={byFile}
      />
      <div className="min-w-0">
        {selected ? (
          <FileDiff
            artifact={artifact}
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
  artifact,
  jobId,
  path,
  status,
}: {
  artifact: Artifact;
  jobId: string;
  path: string;
  status: "added" | "removed" | "modified" | "unchanged";
}) {
  const [base, setBase] = useState<string | null>(null);
  const [head, setHead] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [theme] = useTheme();
  const [baseTokens, setBaseTokens] = useState<HighlightedLines | null>(null);
  const [headTokens, setHeadTokens] = useState<HighlightedLines | null>(null);

  useEffect(() => {
    let abandoned = false;
    setBase(null);
    setHead(null);
    setErr(null);
    setBaseTokens(null);
    setHeadTokens(null);
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

  // Tokenize in a separate effect so a theme flip re-highlights without
  // re-fetching the files.
  useEffect(() => {
    if (base === null || head === null) return;
    let abandoned = false;
    const lang = langForPath(path);
    Promise.all([
      base ? highlight(base, lang, theme) : Promise.resolve<HighlightedLines>([]),
      head ? highlight(head, lang, theme) : Promise.resolve<HighlightedLines>([]),
    ])
      .then(([b, h]) => {
        if (abandoned) return;
        setBaseTokens(b);
        setHeadTokens(h);
      })
      .catch(() => {
        // Highlighter failure is non-fatal; fall through to plain text below.
      });
    return () => {
      abandoned = true;
    };
  }, [base, head, path, theme]);

  // Architectural overlay: for each hunk that points at this file, map its
  // byte span to a line number on the appropriate side. The DiffView gutter
  // uses this to draw a thin accent strip so reviewers can see which lines
  // belong to an emitted Call / State / API hunk.
  const touches = useMemo(() => {
    if (base === null || head === null) return { base: new Map(), head: new Map() };
    const all = hunkTouches(artifact).filter((t) => t.file === path);
    const acc = { base: new Map<number, Set<HunkClass>>(), head: new Map<number, Set<HunkClass>>() };
    for (const t of all) {
      const src = t.side === "base" ? base : head;
      const start = byteToLine(src, t.span.start);
      const end = byteToLine(src, t.span.end);
      const map = acc[t.side];
      for (let ln = start; ln <= end; ln++) {
        const set = map.get(ln) ?? new Set<HunkClass>();
        set.add(t.kind);
        map.set(ln, set);
      }
    }
    return acc;
  }, [artifact, path, base, head]);

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
  const lineTouches: LineTouches = touches;
  return (
    <DiffView
      entries={entries}
      baseTokens={baseTokens}
      headTokens={headTokens}
      touches={lineTouches}
    />
  );
}
