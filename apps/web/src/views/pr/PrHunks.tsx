import type { Artifact, Hunk } from "@/types/artifact";
import { edgeById, nameOf, nodeById } from "@/lib/artifact";

export function PrHunks({ artifact }: { artifact: Artifact }) {
  if (artifact.hunks.length === 0) {
    return (
      <div className="text-[12px] text-muted-foreground">
        No architectural delta — head matches base.
      </div>
    );
  }
  return (
    <ol className="space-y-4">
      {artifact.hunks.map((h, i) => (
        <li key={i}>
          <HunkRow artifact={artifact} hunk={h} />
        </li>
      ))}
    </ol>
  );
}

function HunkRow({ artifact, hunk }: { artifact: Artifact; hunk: Hunk }) {
  return (
    <div className="grid grid-cols-[60px,1fr] gap-3 items-baseline">
      <div className="text-[11px] font-medium text-muted-foreground tracking-wide">
        {hunkLabel(hunk.kind.kind)}
      </div>
      <div>
        <HunkBody artifact={artifact} hunk={hunk} />
      </div>
    </div>
  );
}

function hunkLabel(k: "call" | "state" | "api"): string {
  switch (k) {
    case "call":
      return "Call";
    case "state":
      return "State";
    case "api":
      return "API";
  }
}

function HunkBody({ artifact, hunk }: { artifact: Artifact; hunk: Hunk }) {
  const k = hunk.kind;
  switch (k.kind) {
    case "call":
      return <CallBody artifact={artifact} added={k.added_edges} removed={k.removed_edges} />;
    case "state":
      return (
        <StateBody
          artifact={artifact}
          node={k.node}
          added={k.added_variants}
          removed={k.removed_variants}
        />
      );
    case "api":
      return (
        <ApiBody
          artifact={artifact}
          node={k.node}
          before={k.before_signature ?? null}
          after={k.after_signature ?? null}
        />
      );
  }
}

function CallBody({
  artifact,
  added,
  removed,
}: {
  artifact: Artifact;
  added: number[];
  removed: number[];
}) {
  return (
    <div className="space-y-1">
      {added.map((id) => {
        const e = edgeById(artifact.head, id);
        if (!e) return null;
        return (
          <EdgeLine
            key={`+${id}`}
            mark="+"
            from={nameOf(artifact.head, e.from)}
            to={nameOf(artifact.head, e.to)}
          />
        );
      })}
      {removed.map((id) => {
        const e = edgeById(artifact.base, id);
        if (!e) return null;
        return (
          <EdgeLine
            key={`-${id}`}
            mark="−"
            from={nameOf(artifact.base, e.from)}
            to={nameOf(artifact.base, e.to)}
          />
        );
      })}
    </div>
  );
}

function EdgeLine({ mark, from, to }: { mark: string; from: string; to: string }) {
  return (
    <div className="text-[13px] font-mono flex items-center gap-2">
      <span
        className="w-3 inline-block text-muted-foreground tabular-nums"
        aria-hidden
      >
        {mark}
      </span>
      <span className="text-foreground">{from}</span>
      <span className="text-muted-foreground" aria-hidden>
        →
      </span>
      <span className="text-foreground">{to}</span>
    </div>
  );
}

function StateBody({
  artifact,
  node,
  added,
  removed,
}: {
  artifact: Artifact;
  node: number;
  added: string[];
  removed: string[];
}) {
  const n = nodeById(artifact.head, node) ?? nodeById(artifact.base, node);
  const name = n && "type" in n.kind && n.kind.type === "state" ? n.kind.name : `node ${node}`;
  return (
    <div className="text-[13px] font-mono space-y-1">
      <div className="font-medium text-foreground">{name}</div>
      <div className="flex flex-wrap gap-x-2 gap-y-1">
        {added.map((v) => (
          <Variant key={`+${v}`} mark="+" value={v} />
        ))}
        {removed.map((v) => (
          <Variant key={`-${v}`} mark="−" value={v} />
        ))}
      </div>
    </div>
  );
}

function Variant({ mark, value }: { mark: string; value: string }) {
  return (
    <span className="inline-flex items-center gap-1">
      <span className="text-muted-foreground tabular-nums" aria-hidden>
        {mark}
      </span>
      <span className="text-foreground">{value}</span>
    </span>
  );
}

function ApiBody({
  artifact,
  node,
  before,
  after,
}: {
  artifact: Artifact;
  node: number;
  before: string | null;
  after: string | null;
}) {
  const n = nodeById(artifact.head, node) ?? nodeById(artifact.base, node);
  const name =
    n && "type" in n.kind && n.kind.type === "function" ? n.kind.name : `node ${node}`;
  return (
    <div className="text-[13px] font-mono space-y-1">
      <div className="font-medium text-foreground">{name}</div>
      <div className="space-y-1 text-muted-foreground text-[12px]">
        {before && (
          <div className="flex gap-2">
            <span className="w-3 inline-block tabular-nums" aria-hidden>
              −
            </span>
            <span className="break-all">{before}</span>
          </div>
        )}
        {after && (
          <div className="flex gap-2">
            <span className="w-3 inline-block tabular-nums" aria-hidden>
              +
            </span>
            <span className="text-foreground break-all">{after}</span>
          </div>
        )}
      </div>
    </div>
  );
}
