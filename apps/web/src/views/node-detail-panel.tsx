/** Contextual right panel shown on node click — per RFC cross-cutting rule:
 *  "Shows code, per-node signed cost contribution (three navigation axes),
 *  claims touching that node, and which flows the node participates in."
 *
 *  Opens for a single entity (qualified name). Reads the head/base
 *  source by span, aggregates cost drivers from every flow the entity
 *  participates in, lists claims whose `entities[]` include it, and
 *  links back to the flows it appears in so the reviewer can jump
 *  scope without losing the thread. Closes on ESC / overlay click. */

import { useEffect, useState } from "react";
import type { Artifact, Flow, Node as GraphNode } from "@/types/artifact";
import { fetchFile } from "@/api";
import { signedCostTextClass } from "@/lib/verdict-color";

interface Props {
  artifact: Artifact;
  jobId: string;
  entity: string;
  onClose: () => void;
  onJumpFlow?: (flowId: string) => void;
}

export function NodeDetailPanel({
  artifact,
  jobId,
  entity,
  onClose,
  onJumpFlow,
}: Props) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const headNode = findNodeByName(artifact.head.nodes, entity);
  const baseNode = findNodeByName(artifact.base.nodes, entity);
  const present = headNode ?? baseNode ?? null;
  const status: "added" | "removed" | "changed" | "unchanged" =
    !baseNode && headNode
      ? "added"
      : baseNode && !headNode
        ? "removed"
        : baseNode && headNode && nodeSig(baseNode) !== nodeSig(headNode)
          ? "changed"
          : "unchanged";

  // Flows the entity participates in (including extra_entities set by
  // the LLM assist pass).
  const participatingFlows: Flow[] = (artifact.flows ?? []).filter(
    (f) =>
      (f.entities ?? []).includes(entity) ||
      (f.extra_entities ?? []).includes(entity),
  );

  // Aggregate per-entity cost contribution across every flow it's in.
  // Schema doesn't carry per-entity cost directly; we attribute each
  // flow's cost equally across its entities as a v0 approximation.
  const costAxes = participatingFlows.reduce(
    (acc, f) => {
      if (!f.cost) return acc;
      const denom = (f.entities?.length ?? 0) + (f.extra_entities?.length ?? 0);
      if (denom <= 0) return acc;
      acc.net += f.cost.net / denom;
      acc.continuation += f.cost.axes.continuation / denom;
      acc.runtime += f.cost.axes.runtime / denom;
      acc.operational += f.cost.axes.operational / denom;
      return acc;
    },
    { net: 0, continuation: 0, runtime: 0, operational: 0 },
  );

  // Claims (evidence) across all flows that name this entity.
  const claims = (artifact.flows ?? []).flatMap((f) =>
    (f.evidence ?? [])
      .filter((c) => (c.entities ?? []).includes(entity))
      .map((c) => ({ flowName: f.name, claim: c })),
  );

  return (
    <aside
      role="dialog"
      aria-label={`Details for ${entity}`}
      className="fixed inset-y-0 right-0 z-40 w-[480px] max-w-[92vw] bg-background border-l border-border/70 shadow-2xl flex flex-col"
    >
      <header className="flex items-baseline gap-3 px-4 py-3 border-b border-border/60">
        <div className="min-w-0 flex-1">
          <p className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
            {status} · {present?.file ?? "unknown file"}
          </p>
          <h2 className="text-[13px] font-mono font-semibold text-foreground truncate">
            {entity}
          </h2>
        </div>
        <button
          onClick={onClose}
          className="text-[11px] font-mono text-muted-foreground hover:text-foreground transition-colors"
          title="Close (Esc)"
        >
          close
        </button>
      </header>

      <div className="flex-1 overflow-y-auto p-4 space-y-5">
        <section className="space-y-1.5">
          <h3 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
            Cost contribution · net + three axes
          </h3>
          <div className="grid grid-cols-2 gap-2">
            <CostCell label="net" value={costAxes.net} />
            <CostCell label="continuation" value={costAxes.continuation} />
            <CostCell label="runtime" value={costAxes.runtime} />
            <CostCell label="operational" value={costAxes.operational} />
          </div>
          <p className="text-[10px] text-muted-foreground italic">
            Approximate — per-entity cost attribution is a v0 heuristic (flow cost ÷ entities).
          </p>
        </section>

        <section className="space-y-1.5">
          <h3 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
            In flows ({participatingFlows.length})
          </h3>
          {participatingFlows.length === 0 ? (
            <p className="text-[11px] text-muted-foreground">
              This entity isn't in any flow.
            </p>
          ) : (
            <ul className="flex flex-wrap gap-1.5">
              {participatingFlows.map((f) => (
                <li key={f.id}>
                  <button
                    onClick={() => onJumpFlow?.(f.id)}
                    className="text-[11px] font-mono text-foreground hover:bg-muted/40 border border-border/60 rounded px-2 py-0.5 transition-colors"
                    title={f.rationale}
                  >
                    {f.name}
                  </button>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="space-y-1.5">
          <h3 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
            Claims ({claims.length})
          </h3>
          {claims.length === 0 ? (
            <p className="text-[11px] text-muted-foreground">
              No evidence claims mention this entity.
            </p>
          ) : (
            <ul className="space-y-1">
              {claims.map((c, i) => (
                <li key={i} className="text-[11px] leading-snug">
                  <span className="font-mono text-muted-foreground mr-1.5">
                    {c.claim.kind}
                  </span>
                  <span className="text-foreground">{c.claim.text}</span>
                  <span className="ml-1.5 text-muted-foreground/70">
                    · {c.flowName}
                  </span>
                </li>
              ))}
            </ul>
          )}
        </section>

        <section className="space-y-1.5">
          <h3 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
            Source {headNode ? "· head" : baseNode ? "· base" : ""}
          </h3>
          <SourceSnippet
            jobId={jobId}
            node={present}
            side={headNode ? "head" : "base"}
          />
        </section>
      </div>
    </aside>
  );
}

function CostCell({ label, value }: { label: string; value: number }) {
  const rounded = Math.round(value);
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-2.5 py-1.5">
      <div className="text-[9px] font-mono uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div
        className={
          "text-[16px] font-mono font-semibold tabular-nums " +
          signedCostTextClass(rounded)
        }
      >
        {rounded > 0 ? "+" : rounded < 0 ? "\u2212" : ""}
        {Math.abs(rounded)}
      </div>
    </div>
  );
}

function SourceSnippet({
  jobId,
  node,
  side,
}: {
  jobId: string;
  node: GraphNode | null;
  side: "base" | "head";
}) {
  const [text, setText] = useState<string | null>(null);
  const [err, setErr] = useState<string | null>(null);
  useEffect(() => {
    if (!node) return;
    let cancelled = false;
    setText(null);
    setErr(null);
    fetchFile(jobId, side, node.file)
      .then((body) => {
        if (cancelled) return;
        const slice = body.slice(node.span.start, node.span.end);
        setText(slice.length > 0 ? slice : body);
      })
      .catch((e) => {
        if (cancelled) return;
        setErr(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, [jobId, side, node?.file, node?.span.start, node?.span.end]);

  if (!node) {
    return (
      <p className="text-[11px] text-muted-foreground">
        No source location — the entity isn't in either graph.
      </p>
    );
  }
  if (err) {
    return (
      <p className="text-[11px] text-rose-600 dark:text-rose-300 font-mono">
        {err}
      </p>
    );
  }
  if (text === null) {
    return (
      <p className="text-[11px] text-muted-foreground font-mono">loading…</p>
    );
  }
  return (
    <pre className="text-[11px] font-mono leading-snug whitespace-pre-wrap bg-muted/20 border border-border/60 rounded px-2 py-1.5 max-h-72 overflow-y-auto text-foreground">
      {text}
    </pre>
  );
}

function findNodeByName(nodes: GraphNode[], qname: string): GraphNode | null {
  for (const n of nodes) {
    const k = n.kind as { type: string; name?: string; path?: string };
    if (k.type === "function" || k.type === "type" || k.type === "state") {
      if (k.name === qname) return n;
    } else if (k.type === "api-endpoint" || k.type === "file") {
      if (k.path === qname) return n;
    }
  }
  return null;
}

function nodeSig(n: GraphNode): string {
  const k = n.kind as { type: string; signature?: string; name?: string; path?: string; method?: string };
  if (k.type === "function" && k.signature) return k.signature;
  if (k.type === "api-endpoint") return `${k.method ?? ""} ${k.path ?? ""}`.trim();
  if (k.type === "type" || k.type === "state") return k.name ?? "";
  if (k.type === "file") return k.path ?? "";
  return "";
}
