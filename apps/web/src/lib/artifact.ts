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

export function countFunctions(graph: Artifact["base"]): number {
  return graph.nodes.filter((n) => "type" in n.kind && n.kind.type === "function").length;
}

/** Short, reviewer-friendly PR label from the fixture base/head paths. */
export function deriveSlug(basePath: string, headPath: string): string {
  const norm = (p: string) => p.replace(/\\/g, "/").replace(/\/+$/, "");
  const b = norm(basePath).split("/");
  const h = norm(headPath).split("/");
  // Find the first segment that differs from the end. The segment just before
  // that divergence is the common parent, which is what we want (the fixture
  // slug, e.g. "pr-0004-combined").
  let i = 1;
  while (i <= Math.min(b.length, h.length) && b[b.length - i] === h[h.length - i]) {
    i++;
  }
  return b[b.length - i - 1] ?? "pr";
}

/** Path-like shas (our v0 fixture stand-in) aren't useful to show. Treat
 *  anything starting with a drive letter or `/` as a path. */
export function isPathSha(s: string): boolean {
  return /^[a-zA-Z]:/.test(s) || s.startsWith("/");
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
