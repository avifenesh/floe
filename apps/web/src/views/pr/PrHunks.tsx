import type { Artifact, Hunk } from "@/types/artifact";
import { edgeById, nameOf, nodeById } from "@/lib/artifact";
import { pairSegments, type Segment } from "@/lib/diff";
import { cn } from "@/lib/cn";

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

/* -------------------------------------------------------------------------- */
/* Call                                                                       */
/* -------------------------------------------------------------------------- */

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
    <ul className="space-y-0.5">
      {added.map((id) => {
        const e = edgeById(artifact.head, id);
        if (!e) return null;
        return (
          <EdgeRow
            key={`+${id}`}
            kind="add"
            from={nameOf(artifact.head, e.from)}
            to={nameOf(artifact.head, e.to)}
          />
        );
      })}
      {removed.map((id) => {
        const e = edgeById(artifact.base, id);
        if (!e) return null;
        return (
          <EdgeRow
            key={`-${id}`}
            kind="remove"
            from={nameOf(artifact.base, e.from)}
            to={nameOf(artifact.base, e.to)}
          />
        );
      })}
    </ul>
  );
}

function EdgeRow({
  kind,
  from,
  to,
}: {
  kind: "add" | "remove";
  from: string;
  to: string;
}) {
  return (
    <li
      className={cn(
        "text-[13px] font-mono flex items-center gap-2 px-2 py-0.5 rounded",
        kind === "add" && "bg-emerald-500/30 dark:bg-emerald-400/30 text-emerald-950 dark:text-emerald-50",
        kind === "remove" && "bg-rose-500/30 dark:bg-rose-400/30 text-rose-950 dark:text-rose-50",
      )}
    >
      <span className="w-3 inline-block text-center tabular-nums opacity-70" aria-hidden>
        {kind === "add" ? "+" : "−"}
      </span>
      <span>{from}</span>
      <span className="opacity-60" aria-hidden>
        →
      </span>
      <span>{to}</span>
    </li>
  );
}

/* -------------------------------------------------------------------------- */
/* State                                                                      */
/* -------------------------------------------------------------------------- */

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
    <div className="space-y-2">
      <div className="text-[13px] font-mono font-medium text-foreground">{name}</div>
      <div className="flex flex-wrap gap-1.5">
        {added.map((v) => (
          <VariantChip key={`+${v}`} kind="add" value={v} />
        ))}
        {removed.map((v) => (
          <VariantChip key={`-${v}`} kind="remove" value={v} />
        ))}
      </div>
    </div>
  );
}

function VariantChip({ kind, value }: { kind: "add" | "remove"; value: string }) {
  return (
    <span
      className={cn(
        "inline-flex items-center gap-1 text-[12px] font-mono px-2 py-0.5 rounded",
        kind === "add" && "bg-emerald-500/30 dark:bg-emerald-400/30 text-emerald-950 dark:text-emerald-50",
        kind === "remove" && "bg-rose-500/30 dark:bg-rose-400/30 text-rose-950 dark:text-rose-50",
      )}
    >
      <span className="opacity-70" aria-hidden>
        {kind === "add" ? "+" : "−"}
      </span>
      <span>{value}</span>
    </span>
  );
}

/* -------------------------------------------------------------------------- */
/* API                                                                        */
/* -------------------------------------------------------------------------- */

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

  const [beforeSegs, afterSegs] =
    before && after ? pairSegments(before, after) : [null, null];

  return (
    <div className="space-y-1.5">
      <div className="text-[13px] font-mono font-medium text-foreground">{name}</div>
      <div className="space-y-0.5">
        {before !== null && (
          <SignatureRow kind="remove" text={before} segments={beforeSegs} />
        )}
        {after !== null && (
          <SignatureRow kind="add" text={after} segments={afterSegs} />
        )}
      </div>
    </div>
  );
}

function SignatureRow({
  kind,
  text,
  segments,
}: {
  kind: "add" | "remove";
  text: string;
  segments: Segment[] | null;
}) {
  const hasSegments = !!segments && segments.some((s) => s.kind === "equal");
  return (
    <div
      className={cn(
        "text-[12px] font-mono flex items-start gap-2 px-2 py-0.5 rounded min-w-0",
        kind === "add" &&
          (hasSegments
            ? "bg-emerald-500/[0.06] dark:bg-emerald-400/[0.06]"
            : "bg-emerald-500/30 dark:bg-emerald-400/30 text-emerald-950 dark:text-emerald-50"),
        kind === "remove" &&
          (hasSegments
            ? "bg-rose-500/[0.06] dark:bg-rose-400/[0.06]"
            : "bg-rose-500/30 dark:bg-rose-400/30 text-rose-950 dark:text-rose-50"),
      )}
    >
      <span className="w-3 inline-block text-center tabular-nums text-muted-foreground shrink-0" aria-hidden>
        {kind === "add" ? "+" : "−"}
      </span>
      <span className="whitespace-pre-wrap break-words">
        {segments ? <SegmentedText segments={segments} kind={kind} /> : text}
      </span>
    </div>
  );
}

function SegmentedText({ segments, kind }: { segments: Segment[]; kind: "add" | "remove" }) {
  return (
    <>
      {segments.map((s, i) => {
        if (s.kind === "equal") return <span key={i}>{s.text}</span>;
        return (
          <span
            key={i}
            className={cn(
              "rounded-[2px]",
              kind === "add" &&
                "bg-emerald-500/35 dark:bg-emerald-400/35 text-emerald-950 dark:text-emerald-50",
              kind === "remove" &&
                "bg-rose-500/35 dark:bg-rose-400/35 text-rose-950 dark:text-rose-50",
            )}
          >
            {s.text}
          </span>
        );
      })}
    </>
  );
}
