import { useEffect, useState } from "react";
import {
  compareAnalyses,
  type AnalysisRow,
  type CompareFlow,
  type CompareResponse,
} from "@/api";

/** Compare two analyses head-to-head. Fetches the server-side
 *  comparison payload, renders:
 *    - side headers (headline + repo/sha + flow/hunk count)
 *    - pin match / mismatch banner
 *    - aggregate nav-cost delta (B − A)
 *    - per-flow verdict diff (intent-fit, proof, cost swing)
 *
 *  Opened from a Dashboard sidebar row via shift-click or an explicit
 *  "⇄ compare with…" pair picker. Closed via Esc / backdrop / ×.
 */
export function CompareView({
  a,
  b,
  onClose,
}: {
  a: AnalysisRow;
  b: AnalysisRow;
  onClose: () => void;
}) {
  const [data, setData] = useState<CompareResponse | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let abandoned = false;
    setData(null);
    setErr(null);
    compareAnalyses(a.id, b.id)
      .then((d) => {
        if (!abandoned) setData(d);
      })
      .catch((e) => {
        if (!abandoned) setErr(String(e));
      });
    return () => {
      abandoned = true;
    };
  }, [a.id, b.id]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  return (
    <div
      className="fixed inset-0 z-40 bg-background/60 backdrop-blur-[2px] flex items-start justify-center pt-10 pb-10"
      onClick={onClose}
      role="dialog"
      aria-label="compare analyses"
    >
      <div
        className="rounded-md border border-border/70 bg-background shadow-lg w-[min(92vw,960px)] max-h-[80vh] overflow-y-auto p-4 space-y-4"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-baseline justify-between gap-2">
          <h2 className="text-[14px] font-semibold">Compare analyses</h2>
          <button
            onClick={onClose}
            className="text-[14px] leading-none text-muted-foreground hover:text-foreground"
            aria-label="close"
          >
            ×
          </button>
        </header>
        {err ? (
          <p className="text-[12px] font-mono text-destructive">
            Compare failed: {err}
          </p>
        ) : !data ? (
          <p className="text-[12px] text-muted-foreground">Loading…</p>
        ) : (
          <CompareBody data={data} />
        )}
      </div>
    </div>
  );
}

function CompareBody({ data }: { data: CompareResponse }) {
  return (
    <div className="space-y-4">
      <div className="grid grid-cols-2 gap-3">
        <SideHeader label="A" side={data.a} />
        <SideHeader label="B" side={data.b} />
      </div>
      <PinBanner matches={data.pin_matches} />
      {data.aggregate_delta && <AggregateRow delta={data.aggregate_delta} />}
      <FlowDiff flows={data.flows} />
    </div>
  );
}

function SideHeader({
  label,
  side,
}: {
  label: string;
  side: CompareResponse["a"];
}) {
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 space-y-0.5">
      <div className="flex items-baseline gap-2">
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
          {label}
        </span>
        <span className="text-[11px] font-mono text-foreground/70 truncate">
          {side.repo}
        </span>
      </div>
      <p className="text-[13px] font-semibold text-foreground truncate">
        {side.headline ?? side.head_sha.slice(0, 8)}
      </p>
      <p className="text-[10px] font-mono text-muted-foreground">
        {side.flow_count} flow{side.flow_count === 1 ? "" : "s"} · {side.hunk_count}{" "}
        hunk{side.hunk_count === 1 ? "" : "s"} · synth {side.synth_status} · proof{" "}
        {side.proof_status}
      </p>
      {side.verdict && (
        <p className="text-[10px] font-mono text-muted-foreground">
          verdict: {side.verdict.verdict} (by {side.verdict.author})
        </p>
      )}
    </div>
  );
}

function PinBanner({ matches }: { matches: boolean }) {
  return (
    <div
      className={
        "rounded border px-3 py-1.5 text-[11px] font-mono " +
        (matches
          ? "border-emerald-400/50 bg-emerald-50 text-emerald-900 dark:bg-emerald-400/10 dark:text-emerald-200"
          : "border-amber-400/50 bg-amber-50 text-amber-900 dark:bg-amber-400/10 dark:text-amber-200")
      }
    >
      {matches
        ? "pin matches — apples-to-apples."
        : "pin mismatch — baseline, synthesis, or proof model differs. Deltas below are directional, not precise."}
    </div>
  );
}

function AggregateRow({
  delta,
}: {
  delta: NonNullable<CompareResponse["aggregate_delta"]>;
}) {
  const signed = (n: number) => (n > 0 ? `+${n}` : `${n}`);
  return (
    <section className="space-y-1">
      <h3 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
        Aggregate nav-cost delta (B − A)
      </h3>
      <div className="grid grid-cols-4 gap-2 text-[11px] font-mono">
        <Stat label="continuation" value={signed(delta.continuation)} />
        <Stat label="runtime" value={signed(delta.runtime)} />
        <Stat label="operational" value={signed(delta.operational)} />
        <Stat label="tokens" value={signed(delta.tokens)} />
      </div>
    </section>
  );
}

function Stat({ label, value }: { label: string; value: string }) {
  return (
    <div className="rounded border border-border/60 bg-background px-2 py-1">
      <p className="text-[10px] uppercase tracking-wide text-muted-foreground">
        {label}
      </p>
      <p className="text-[13px] font-semibold tabular-nums">{value}</p>
    </div>
  );
}

function FlowDiff({ flows }: { flows: CompareFlow[] }) {
  if (flows.length === 0) {
    return (
      <p className="text-[12px] text-muted-foreground italic">
        No flows on either side.
      </p>
    );
  }
  return (
    <section className="space-y-1">
      <h3 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
        Per-flow verdict diff ({flows.length})
      </h3>
      <ul className="divide-y divide-border/50 rounded border border-border/50">
        {flows.map((f) => (
          <li key={f.name + f.presence} className="px-3 py-2 space-y-1">
            <div className="flex items-baseline gap-2">
              <span className="text-[12px] font-mono text-foreground truncate">
                {f.name}
              </span>
              <PresenceBadge presence={f.presence} />
            </div>
            <div className="grid grid-cols-2 gap-2 text-[11px] font-mono text-muted-foreground">
              <FlowSide label="A" side={f.a} />
              <FlowSide label="B" side={f.b} />
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

function PresenceBadge({ presence }: { presence: CompareFlow["presence"] }) {
  if (presence === "both") return null;
  const tone =
    presence === "only-a"
      ? "border-rose-400/50 bg-rose-50 text-rose-900 dark:bg-rose-400/10 dark:text-rose-200"
      : "border-emerald-400/50 bg-emerald-50 text-emerald-900 dark:bg-emerald-400/10 dark:text-emerald-200";
  return (
    <span className={"text-[9px] font-mono uppercase px-1.5 py-0.5 rounded-full border " + tone}>
      {presence === "only-a" ? "only in A" : "only in B"}
    </span>
  );
}

function FlowSide({
  label,
  side,
}: {
  label: string;
  side: CompareFlow["a"];
}) {
  if (!side) {
    return (
      <div className="opacity-60">
        <span className="text-[9px] uppercase tracking-wide">{label}</span>{" "}
        <span>—</span>
      </div>
    );
  }
  return (
    <div>
      <span className="text-[9px] uppercase tracking-wide text-muted-foreground">
        {label}
      </span>{" "}
      <span>fit: {side.intent_fit ?? "—"}</span>
      {" · "}
      <span>proof: {side.proof ?? "—"}</span>
      {side.cost_net !== null && (
        <>
          {" · "}
          <span>cost: {side.cost_net > 0 ? `+${side.cost_net}` : side.cost_net}</span>
        </>
      )}
    </div>
  );
}
