import type { FlowMembership, MembershipMember } from "@/types/artifact";

/** Render the LLM-curated flow membership as an actual graph.
 *
 *  Three horizontal rows — entrance / core / exit — so the eye reads
 *  top-to-bottom as "call comes in, flows through the middle, leaves
 *  at the bottom." Edges drawn as SVG paths; shape overlays
 *  annotate loops / branches / fanouts on top of the base edge set.
 *
 *  Intentionally simple: fixed node width, deterministic layout,
 *  no auto-routing. Good enough for membership's ≤10-node budget.
 *  Per-member `why` surfaces on hover as native title tooltip. */
export function MembershipGraph({
  membership,
  onNodeClick,
}: {
  membership: FlowMembership;
  onNodeClick?: (entity: string) => void;
}) {
  const byRole = (r: string) =>
    (membership.members ?? []).filter((m) => (m.role ?? "") === r);
  const entrance = byRole("entrance");
  const core = byRole("core");
  const exit = byRole("exit");
  const other = (membership.members ?? []).filter(
    (m) => !["entrance", "core", "exit"].includes(m.role ?? ""),
  );
  // Everything without a role → treat as core so it gets rendered
  // (the model sometimes omits role).
  const coreAll = [...core, ...other];

  // Layout: fixed node width/height + row gap. Columns derived per
  // row. Total width = max row's columns. `viewBox` scales the
  // final SVG to the container.
  const NODE_W = 160;
  const NODE_H = 44;
  const COL_GAP = 24;
  const ROW_GAP = 56;
  const PAD = 20;
  const rows: { role: string; items: MembershipMember[] }[] = [
    { role: "entrance", items: entrance },
    { role: "core", items: coreAll },
    { role: "exit", items: exit },
  ].filter((r) => r.items.length > 0);
  if (rows.length === 0) return null;

  const maxCols = rows.reduce((m, r) => Math.max(m, r.items.length), 1);
  const totalW = PAD * 2 + maxCols * NODE_W + (maxCols - 1) * COL_GAP;
  const totalH = PAD * 2 + rows.length * NODE_H + (rows.length - 1) * ROW_GAP;

  // Compute per-entity position for edge drawing.
  const pos = new Map<string, { x: number; y: number; role: string }>();
  rows.forEach((row, rIdx) => {
    const rowW =
      row.items.length * NODE_W + (row.items.length - 1) * COL_GAP;
    const rowX = (totalW - rowW) / 2;
    row.items.forEach((it, cIdx) => {
      pos.set(it.entity, {
        x: rowX + cIdx * (NODE_W + COL_GAP),
        y: PAD + rIdx * (NODE_H + ROW_GAP),
        role: row.role,
      });
    });
  });

  const edges = membership.edges ?? [];
  const shapes = membership.shapes ?? [];
  // Shape-edge dedup: if an edge is part of a loop shape, draw it
  // with the loop style rather than as two flat edges.
  const loopPairs = new Set<string>();
  for (const s of shapes) {
    const nodes = s.nodes ?? [];
    if (s.kind === "loop" && nodes.length >= 2) {
      for (let i = 0; i < nodes.length; i++) {
        const a = nodes[i]!;
        const b = nodes[(i + 1) % nodes.length]!;
        loopPairs.add(`${a}→${b}`);
        loopPairs.add(`${b}→${a}`);
      }
    }
  }

  return (
    <div className="overflow-x-auto rounded-md border border-border/60 bg-muted/60 shadow-sm">
      <svg
        viewBox={`0 0 ${totalW} ${totalH}`}
        className="block w-full"
        preserveAspectRatio="xMidYMid meet"
        style={{ minHeight: totalH, maxHeight: totalH * 2 }}
      >
        <defs>
          <marker
            id="mg-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-muted-foreground/70" />
          </marker>
          <marker
            id="mg-arrow-loop"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto-start-reverse"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-amber-500/80" />
          </marker>
        </defs>

        {/* Row labels on the left — subtle. */}
        {rows.map((row, rIdx) => (
          <text
            key={`lbl-${row.role}`}
            x={4}
            y={PAD + rIdx * (NODE_H + ROW_GAP) + NODE_H / 2 + 3}
            className="fill-muted-foreground"
            fontSize={9}
            fontFamily="monospace"
            textAnchor="start"
          >
            {row.role}
          </text>
        ))}

        {/* Edges first, so nodes draw over them. */}
        {edges.map((e, i) => {
          const a = pos.get(e.from);
          const b = pos.get(e.to);
          if (!a || !b) return null;
          const x1 = a.x + NODE_W / 2;
          const y1 = a.y + NODE_H;
          const x2 = b.x + NODE_W / 2;
          const y2 = b.y;
          const isLoop = loopPairs.has(`${e.from}→${e.to}`);
          const midY = (y1 + y2) / 2;
          const path = `M ${x1},${y1} C ${x1},${midY} ${x2},${midY} ${x2},${y2}`;
          return (
            <g key={`e-${i}`}>
              <path
                d={path}
                fill="none"
                strokeWidth={isLoop ? 1.6 : 1.2}
                className={
                  isLoop
                    ? "stroke-amber-500/70"
                    : e.kind === "data-flow"
                      ? "stroke-muted-foreground/50"
                      : "stroke-muted-foreground/70"
                }
                strokeDasharray={e.kind === "data-flow" ? "3 3" : undefined}
                markerEnd={isLoop ? "url(#mg-arrow-loop)" : "url(#mg-arrow)"}
              />
              {e.note && (
                <text
                  x={(x1 + x2) / 2 + 4}
                  y={midY - 2}
                  className="fill-muted-foreground/90"
                  fontSize={8}
                  fontFamily="monospace"
                >
                  <title>{e.note}</title>
                  {e.note.length > 24 ? e.note.slice(0, 22) + "…" : e.note}
                </text>
              )}
            </g>
          );
        })}

        {/* Self-loop back-arrows for loops declared as `loop` shape. */}
        {shapes
          .filter((s) => s.kind === "loop" && (s.nodes ?? []).length >= 2)
          .flatMap((s, si) => {
            const nodes = s.nodes ?? [];
            const segs: JSX.Element[] = [];
            // Back-edge from tail to head, drawn as a curved line
            // hugging the right side.
            const first = pos.get(nodes[0]!);
            const last = pos.get(nodes[nodes.length - 1]!);
            if (!first || !last) return segs;
            const x1 = last.x + NODE_W;
            const y1 = last.y + NODE_H / 2;
            const x2 = first.x + NODE_W;
            const y2 = first.y + NODE_H / 2;
            const controlX = Math.max(x1, x2) + 48;
            const path = `M ${x1},${y1} C ${controlX},${y1} ${controlX},${y2} ${x2},${y2}`;
            segs.push(
              <path
                key={`loop-${si}`}
                d={path}
                fill="none"
                strokeWidth={1.4}
                strokeDasharray="4 3"
                className="stroke-amber-500/70"
                markerEnd="url(#mg-arrow-loop)"
              />,
            );
            return segs;
          })}

        {/* Nodes. */}
        {rows.flatMap((row) =>
          row.items.map((m) => {
            const p = pos.get(m.entity);
            if (!p) return null;
            const tone =
              row.role === "entrance"
                ? "stroke-sky-500/70 fill-sky-500/10"
                : row.role === "exit"
                  ? "stroke-violet-500/70 fill-violet-500/10"
                  : "stroke-foreground/40 fill-muted/40";
            return (
              <g
                key={`n-${m.entity}`}
                transform={`translate(${p.x},${p.y})`}
                onClick={onNodeClick ? () => onNodeClick(m.entity) : undefined}
                style={{ cursor: onNodeClick ? "pointer" : "default" }}
              >
                <title>{m.why || m.entity}</title>
                <rect
                  width={NODE_W}
                  height={NODE_H}
                  rx={6}
                  strokeWidth={1.2}
                  className={tone}
                />
                <text
                  x={NODE_W / 2}
                  y={NODE_H / 2 - 2}
                  textAnchor="middle"
                  className="fill-foreground"
                  fontSize={11}
                  fontFamily="monospace"
                  fontWeight={500}
                >
                  {m.entity.length > 22 ? m.entity.slice(0, 20) + "…" : m.entity}
                </text>
                {m.side && (
                  <text
                    x={NODE_W / 2}
                    y={NODE_H / 2 + 11}
                    textAnchor="middle"
                    className="fill-muted-foreground"
                    fontSize={8}
                    fontFamily="monospace"
                  >
                    {m.side}
                  </text>
                )}
              </g>
            );
          }),
        )}
      </svg>
    </div>
  );
}
