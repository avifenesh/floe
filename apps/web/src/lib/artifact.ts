import type { Artifact, Node, Edge, NodeId } from "@/types/artifact";

/** Given a node id, return a human-readable name or "?" if unknown. */
export function nameOf(graph: Artifact["base"], id: NodeId): string {
  const n = graph.nodes.find((x) => x.id === id);
  if (!n) return "?";
  const k = n.kind;
  if ("type" in k) {
    switch (k.type) {
      case "function":
        return k.name;
      case "type":
      case "state":
        return k.name;
      case "api-endpoint":
        return `${k.method} ${k.path}`;
      case "file":
        return k.path;
    }
  }
  return "?";
}

/** Lookup the Edge for an id in a graph. */
export function edgeById(graph: Artifact["base"], id: number): Edge | undefined {
  return graph.edges.find((e) => e.id === id);
}

/** All File nodes across both sides, deduped by path. */
export function filesTouched(a: Artifact): string[] {
  const paths = new Set<string>();
  for (const n of [...a.base.nodes, ...a.head.nodes]) {
    if ("type" in n.kind && n.kind.type === "file") paths.add(n.kind.path);
  }
  return Array.from(paths).sort();
}

export type ChangedFile = {
  path: string;
  /** `added` = only in head · `removed` = only in base · `modified` = both sides
   *  with different File-node provenance hashes · `unchanged` = both sides, equal. */
  status: "added" | "removed" | "modified" | "unchanged";
};

/** Walk File nodes on both sides; pair by path. Modified = hash mismatch. */
export function changedFiles(a: Artifact): ChangedFile[] {
  const baseByPath = new Map<string, string>();
  const headByPath = new Map<string, string>();
  for (const n of a.base.nodes) {
    if ("type" in n.kind && n.kind.type === "file") {
      baseByPath.set(n.kind.path, n.provenance.hash);
    }
  }
  for (const n of a.head.nodes) {
    if ("type" in n.kind && n.kind.type === "file") {
      headByPath.set(n.kind.path, n.provenance.hash);
    }
  }
  const paths = new Set<string>([...baseByPath.keys(), ...headByPath.keys()]);
  const out: ChangedFile[] = [];
  for (const path of paths) {
    const b = baseByPath.get(path);
    const h = headByPath.get(path);
    if (b && !h) out.push({ path, status: "removed" });
    else if (!b && h) out.push({ path, status: "added" });
    else if (b !== h) out.push({ path, status: "modified" });
    else out.push({ path, status: "unchanged" });
  }
  return out.sort((x, y) => x.path.localeCompare(y.path));
}

export function countFunctions(graph: Artifact["base"]): number {
  return graph.nodes.filter((n) => "type" in n.kind && n.kind.type === "function").length;
}

/** Per-type hunk counts across an artifact or a scoped hunk list. */
export function hunkTypeCounts(
  hunks: Artifact["hunks"],
): { call: number; state: number; api: number; total: number } {
  let call = 0;
  let state = 0;
  let api = 0;
  for (const h of hunks) {
    if (h.kind.kind === "call") call++;
    else if (h.kind.kind === "state") state++;
    else if (h.kind.kind === "api") api++;
  }
  return { call, state, api, total: call + state + api };
}

/** Hunk objects belonging to a flow (by id). */
export function flowHunks(a: Artifact, hunkIds: string[]): Artifact["hunks"] {
  const wanted = new Set(hunkIds);
  return a.hunks.filter((h) => wanted.has(h.id));
}

/** File paths a single hunk touches (caller file for call hunks, defining
 *  file for state/api). Unions both sides when node provenance differs. */
export function filesOfHunk(a: Artifact, hunkId: string): string[] {
  const h = a.hunks.find((x) => x.id === hunkId);
  if (!h) return [];
  const files = new Set<string>();
  const k = h.kind;
  if (k.kind === "call") {
    for (const id of k.added_edges) {
      const e = edgeById(a.head, id);
      const n = e && nodeById(a.head, e.from);
      if (n) files.add(n.file);
    }
    for (const id of k.removed_edges) {
      const e = edgeById(a.base, id);
      const n = e && nodeById(a.base, e.from);
      if (n) files.add(n.file);
    }
  } else if (
    k.kind === "lock" ||
    k.kind === "data" ||
    k.kind === "docs" ||
    k.kind === "deletion"
  ) {
    files.add(k.file);
  } else {
    const hn = nodeById(a.head, k.node);
    if (hn) files.add(hn.file);
    const bn = nodeById(a.base, k.node);
    if (bn) files.add(bn.file);
  }
  return Array.from(files);
}

/** Per-file hunk count across the whole artifact. A hunk that touches two
 *  files counts once per file. */
export function hunkCountByFile(a: Artifact): Map<string, number> {
  const out = new Map<string, number>();
  for (const h of a.hunks) {
    for (const file of filesOfHunk(a, h.id)) {
      out.set(file, (out.get(file) ?? 0) + 1);
    }
  }
  return out;
}

/** Per-file flow list — which flows participate in this file. */
export function flowsByFile(a: Artifact): Map<string, import("@/types/artifact").Flow[]> {
  const flows = a.flows ?? [];
  const acc = new Map<string, Set<string>>();
  const byId = new Map(flows.map((f) => [f.id, f] as const));
  for (const f of flows) {
    for (const hid of f.hunk_ids) {
      for (const file of filesOfHunk(a, hid)) {
        const s = acc.get(file) ?? new Set<string>();
        s.add(f.id);
        acc.set(file, s);
      }
    }
  }
  const out = new Map<string, import("@/types/artifact").Flow[]>();
  for (const [file, ids] of acc) {
    out.set(
      file,
      Array.from(ids)
        .map((id) => byId.get(id))
        .filter((x): x is import("@/types/artifact").Flow => !!x),
    );
  }
  return out;
}

/** Short, reviewer-friendly PR label from the fixture base/head paths. */
export function deriveSlug(basePath: string, headPath: string): string {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const b = norm(basePath).split("/");
  const h = norm(headPath).split("/");

  // Primary: the last segments usually carry the repo + pr id in a
  // `<repo>-base-<id>` / `<repo>-head-<id>` shape (git worktree convention we
  // use in dev). Extract the surrounding prefix + suffix and render `repo #id`.
  const leaf = tryBaseHeadLeafPattern(b[b.length - 1], h[h.length - 1]);
  if (leaf) return leaf;

  // Secondary: common parent dir one level up (useful when the worktree names
  // don't encode the pr id but the shared parent is meaningful, e.g. a fixture
  // slug like "pr-0004-combined").
  let i = 1;
  while (i <= Math.min(b.length, h.length) && b[b.length - i] === h[h.length - i]) {
    i++;
  }
  const parent = b[b.length - i - 1];
  if (parent && parent !== "Temp" && parent !== "tmp" && parent !== "var") {
    return parent;
  }

  // Fallback: show both leaf names — honest about having no slug.
  return `${b[b.length - 1]} ↔ ${h[h.length - 1]}`;
}

/** If `<prefix>base<suffix>` / `<prefix>head<suffix>` pattern, return
 *  `<clean-prefix> #<clean-suffix>` (or just `<prefix>` if no suffix). */
function tryBaseHeadLeafPattern(base: string | undefined, head: string | undefined): string | null {
  if (!base || !head || base === head) return null;
  // Longest common prefix + suffix.
  let pre = 0;
  while (pre < base.length && pre < head.length && base[pre] === head[pre]) pre++;
  let suf = 0;
  while (
    suf < base.length - pre &&
    suf < head.length - pre &&
    base[base.length - 1 - suf] === head[head.length - 1 - suf]
  ) {
    suf++;
  }
  const baseMid = base.slice(pre, base.length - suf);
  const headMid = head.slice(pre, head.length - suf);
  if (baseMid !== "base" || headMid !== "head") return null;
  const prefix = base.slice(0, pre).replace(/[-_]+$/, "");
  const suffix = base.slice(base.length - suf).replace(/^[-_]+/, "");
  if (!prefix) return null;
  return suffix ? `${prefix} #${suffix}` : prefix;
}

/** Path-like shas (our v0 fixture stand-in) aren't useful to show. Treat
 *  anything starting with a drive letter, `/`, or Windows UNC prefix as a path. */
export function isPathSha(s: string): boolean {
  return (
    /^[a-zA-Z]:/.test(s) ||
    s.startsWith("/") ||
    s.startsWith("\\\\") ||
    s.startsWith("\\?\\")
  );
}

/** Shorten a long path or sha into a 12-char tail. */
export function shortSha(s: string): string {
  const norm = s.replace(/\\/g, "/");
  const seg = norm.split("/").pop() ?? norm;
  return seg.length > 12 ? seg.slice(0, 12) + "…" : seg;
}

/** A node by id, or undefined. */
export function nodeById(graph: Artifact["base"], id: NodeId): Node | undefined {
  return graph.nodes.find((n) => n.id === id);
}

export type HunkClass = "call" | "state" | "api";

export interface HunkTouch {
  /** Which snapshot this touch refers to. */
  side: "base" | "head";
  file: string;
  /** Byte range in the side's file. */
  span: { start: number; end: number };
  kind: HunkClass;
}

/**
 * Derive per-side, per-file byte ranges that an emitted hunk points at.
 * The Source view renders these as a gutter strip so a reviewer can see
 * "this line is part of the Call/State/API hunk the PR view lists".
 *
 * For Call hunks the resolved range is the *caller* function's span —
 * the callsite itself has no explicit span in the schema, but every
 * call originates inside the caller's body.
 */
export function hunkTouches(artifact: Artifact): HunkTouch[] {
  const out: HunkTouch[] = [];
  for (const h of artifact.hunks) {
    const k = h.kind;
    switch (k.kind) {
      case "call": {
        for (const id of k.added_edges) {
          const e = edgeById(artifact.head, id);
          if (!e) continue;
          const caller = nodeById(artifact.head, e.from);
          if (caller) push(out, "head", caller, "call");
        }
        for (const id of k.removed_edges) {
          const e = edgeById(artifact.base, id);
          if (!e) continue;
          const caller = nodeById(artifact.base, e.from);
          if (caller) push(out, "base", caller, "call");
        }
        break;
      }
      case "state": {
        const headN = nodeById(artifact.head, k.node);
        if (headN) push(out, "head", headN, "state");
        const baseN = nodeById(artifact.base, k.node);
        if (baseN && baseN !== headN) push(out, "base", baseN, "state");
        break;
      }
      case "api": {
        const headN = nodeById(artifact.head, k.node);
        if (headN) push(out, "head", headN, "api");
        const baseN = nodeById(artifact.base, k.node);
        if (baseN && baseN !== headN) push(out, "base", baseN, "api");
        break;
      }
    }
  }
  return out;
}

function push(out: HunkTouch[], side: "base" | "head", n: Node, kind: HunkClass) {
  out.push({ side, file: n.file, span: { start: n.span.start, end: n.span.end }, kind });
}

/** Convert a 0-based byte offset into a 1-based line number. */
export function byteToLine(content: string, byte: number): number {
  let line = 1;
  const end = Math.min(byte, content.length);
  for (let i = 0; i < end; i++) {
    if (content.charCodeAt(i) === 10) line++;
  }
  return line;
}
