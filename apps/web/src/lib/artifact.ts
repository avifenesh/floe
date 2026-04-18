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
