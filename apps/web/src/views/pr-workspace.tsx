import { useState } from "react";
import type { Artifact, Flow } from "@/types/artifact";
import { PR_SUB_TABS, type PrSubTab, type TopTab } from "./types";
import { SlideSwitch } from "@/components/SlideSwitch";
import { intentFitPillClass, proofPillClass, signedCostTextClass } from "@/lib/verdict-color";
import { aggregateCostConfidence, CONFIDENCE_THRESHOLD } from "@/lib/cost-confidence";
import { PrFlows } from "./pr/PrFlows";
import { PrHeader } from "./pr/PrHeader";
import { PrStats } from "./pr/PrStats";
import { PrHunks } from "./pr/PrHunks";
import { SourceView } from "./source";
import { BaselineDriftBanner } from "@/components/BaselineDriftBanner";

interface Props {
  artifact: Artifact;
  jobId: string;
  sub: PrSubTab;
  onTop: (t: TopTab) => void;
  onRebaseline?: () => void;
  rebaselining?: boolean;
}

/**
 * Whole-PR workspace. Sub-tabs:
 *   flows-map — overview of detected flows; click a flow card to open its
 *     top-tab workspace.
 *   diff — the full textual diff, unscoped (the existing Source view).
 *   cost — aggregate PR cost: axes, tokens, per-flow contribution.
 *   proof — intent + per-flow proof verdicts with claim breakdown.
 *   meta — identity header + stats + raw hunk list.
 */
export function PrWorkspace({ artifact, jobId, sub, onTop, onRebaseline, rebaselining }: Props) {
  const order = PR_SUB_TABS.findIndex((t) => t.key === sub);
  const body = (() => {
    switch (sub) {
      case "flows-map":
        return <FlowsMap artifact={artifact} onTop={onTop} />;
      case "diff":
        return <SourceView artifact={artifact} jobId={jobId} />;
      case "cost":
        return <PrCost artifact={artifact} />;
      case "proof":
        return <PrProof artifact={artifact} onTop={onTop} />;
      case "meta":
        return <Meta artifact={artifact} />;
    }
  })();
  return (
    <>
      <BaselineDriftBanner
        artifact={artifact}
        onRebaseline={onRebaseline}
        rebaselining={rebaselining}
      />
      <SlideSwitch viewKey={`pr-${sub}`} order={order}>
        {body}
      </SlideSwitch>
    </>
  );
}

function FlowsMap({
  artifact,
  onTop,
}: {
  artifact: Artifact;
  onTop: (t: TopTab) => void;
}) {
  return (
    <div className="space-y-5">
      <PrFlows
        artifact={artifact}
        onPick={(flowId) => onTop({ kind: "flow", flowId })}
      />
    </div>
  );
}

function Meta({ artifact }: { artifact: Artifact }) {
  return (
    <div className="space-y-6">
      <PrHeader artifact={artifact} />
      <PrStats artifact={artifact} />
      <section className="space-y-4">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Architectural delta
        </h2>
        <PrHunks artifact={artifact} />
      </section>
    </div>
  );
}


function PrCost({ artifact }: { artifact: Artifact }) {
  const status = artifact.cost_status ?? "not-run";
  const flows = artifact.flows ?? [];

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">
          PR cost · analysing
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Probing the repo. Aggregate cost fills in once the base + head
          probes complete; per-flow breakdown shows up at the same time.
        </p>
      </div>
    );
  }

  if (status === "not-run") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">PR cost</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Cost unavailable — the probe pass isn't configured. Set
          <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">ADR_PROBE_LLM</code>
          and re-run.
        </p>
      </div>
    );
  }

  if (status === "errored") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">
          PR cost · errored
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Probe pass failed. Aggregate may be missing or partial. Server
          logs hold the specific failure.
        </p>
      </div>
    );
  }

  // Sum per-flow into a PR-aggregate. Flows without a `cost` contribute 0.
  const totals = flows.reduce(
    (acc, f) => {
      const c = f.cost;
      if (!c) return acc;
      acc.net += c.net;
      acc.axes.continuation += c.axes.continuation;
      acc.axes.runtime += c.axes.runtime;
      acc.axes.operational += c.axes.operational;
      acc.tokensDelta += c.tokens_delta ?? 0;
      return acc;
    },
    {
      net: 0,
      axes: { continuation: 0, runtime: 0, operational: 0 },
      tokensDelta: 0,
    },
  );

  const baseline = artifact.baseline ?? null;
  // Headline percent — sum of |axis| over all axes, divided by sum of
  // baseline denominators. Communicates "repo navigation cost moved by X%".
  const baselineAxesSum = baseline
    ? baseline.axes_base.continuation +
      baseline.axes_base.runtime +
      baseline.axes_base.operational
    : 0;
  const netPct =
    baselineAxesSum > 0
      ? (totals.net / baselineAxesSum) * 100
      : null;
  const tokensPct =
    baseline && baseline.tokens_base > 0
      ? (totals.tokensDelta / baseline.tokens_base) * 100
      : null;

  const flowCosts = flows
    .filter((f) => f.cost != null)
    .map((f) => ({ name: f.name, net: f.cost!.net }))
    .sort((a, b) => Math.abs(b.net) - Math.abs(a.net));

  // Probe model stamp only lives in BaselineStamp below (diagnostic
  // metadata), not in the hero strip — no model names in the hot
  // copy per feedback_no_model_names_in_ui.
  const confidence = aggregateCostConfidence(flows.map((f) => f.cost ?? null));
  const lowConfidence = confidence < CONFIDENCE_THRESHOLD;
  // When confidence is low, the net is unreliable — grey it out and
  // surface a hint pointing at the per-flow drivers below. Drivers
  // themselves stay fully visible (they're the trustworthy bit).
  const heroColor = lowConfidence
    ? "text-muted-foreground"
    : signedCostTextClass(netPct ?? totals.net);

  return (
    <section className="space-y-4">
      <header className="space-y-3">
        <div className="flex items-baseline gap-4">
          {netPct !== null ? (
            <>
              <span
                className={
                  "text-[22px] font-mono font-semibold tabular-nums leading-none " +
                  heroColor
                }
              >
                {formatPctHero(netPct)}
              </span>
              <span className="text-[14px] font-mono tabular-nums text-muted-foreground">
                <span className={lowConfidence ? "" : signedCostTextClass(totals.net)}>
                  {formatSigned(totals.net)}
                </span>{" "}
                nav-cost units · of baseline
              </span>
            </>
          ) : (
            <span
              className={
                "text-[22px] font-mono font-semibold tabular-nums leading-none " +
                heroColor
              }
            >
              {formatSigned(totals.net)}
            </span>
          )}
          <span className="text-[12px] text-muted-foreground">
            PR navigation-cost delta · summed across flows
          </span>
          {lowConfidence && (
            <span
              title={`Aggregated driver confidence ${(confidence * 100).toFixed(0)}% (< ${(CONFIDENCE_THRESHOLD * 100).toFixed(0)}%) — read the per-flow drivers below; the headline net is unreliable.`}
              className="ml-auto text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-amber-400/40 bg-amber-50 dark:bg-amber-400/10 text-amber-800 dark:text-amber-200"
            >
              low confidence
            </span>
          )}
        </div>
        <AxisRow axes={totals.axes} baseline={baseline?.axes_base ?? null} />
        <TokensRow delta={totals.tokensDelta} pct={tokensPct} />
        <BaselineStamp baseline={baseline} />
      </header>
      <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
        Per-flow contribution ({flowCosts.length})
      </h2>
      <ol className="space-y-2">
        {flowCosts.map((f) => (
          <li key={f.name}>
            <FlowContribRow
              name={f.name}
              net={f.net}
              denom={baselineAxesSum}
            />
          </li>
        ))}
      </ol>
    </section>
  );
}

/** Baseline-pin stamp — "what produced this number." Shows the
 *  probe + synthesis + proof models so a reviewer comparing two
 *  analyses across model swaps can SEE the pin instead of guessing.
 *  The refusal-on-drift half of the RFC v0.3 §9 contract lives in
 *  the `adr baseline --against` CLI; this strip is the visible-pin
 *  half. (`baselinePinMatches` in `lib/baseline-pin.ts` mirrors the
 *  Rust check for when an in-UI compare view arrives.) */
function BaselineStamp({
  baseline,
}: {
  baseline: import("@/types/artifact").ArtifactBaseline | null;
}) {
  if (!baseline) return null;
  // RFC v0.3 §9: pin spans probe + synthesis + proof so two runs
  // with different LLMs never get silently compared. Render all
  // three so the reviewer can see what produced these numbers.
  return (
    <div
      className="flex flex-wrap items-baseline gap-x-2 gap-y-0.5 text-[10px] font-mono text-muted-foreground"
      title="Baseline pin: identifies exactly which (probe, synthesis, proof) models produced the cost + proof numbers above. Re-baselines on any field change."
    >
      <span className="uppercase tracking-wide opacity-70">pinned to</span>
      <span className="text-foreground/80">probe {baseline.probe_model}</span>
      <span className="opacity-50">·</span>
      <span>probes {baseline.probe_set_version}</span>
      {baseline.synthesis_model && (
        <>
          <span className="opacity-50">·</span>
          <span className="text-foreground/80">synth {baseline.synthesis_model}</span>
        </>
      )}
      {baseline.proof_model && (
        <>
          <span className="opacity-50">·</span>
          <span className="text-foreground/80">proof {baseline.proof_model}</span>
        </>
      )}
    </div>
  );
}

function pctOfBaseline(value: number, denom: number | null | undefined): number {
  if (!denom || denom <= 0) return 0;
  return Math.min(100, (Math.abs(value) / denom) * 100);
}

function formatPct(n: number): string {
  const sign = n < 0 ? "\u2212" : n > 0 ? "+" : "";
  return `${sign}${Math.abs(n).toFixed(1)}%`;
}

/** Hero rendition of a percentage — same number as `formatPct` but
 *  the `%` glyph is shrunk to ~62% so the digit dominates. Used only
 *  in oversized headers; small inline pcts keep the plain string. */
function formatPctHero(n: number): import("react").ReactNode {
  const sign = n < 0 ? "\u2212" : n > 0 ? "+" : "";
  const abs = Math.abs(n).toFixed(1);
  return (
    <>
      {sign}
      {abs}
      <span className="text-[0.62em] align-baseline ml-[0.05em] opacity-80">%</span>
    </>
  );
}

function TokensRow({
  delta,
  pct,
}: {
  delta: number;
  pct: number | null;
}) {
  const width = pct === null ? 0 : Math.min(100, Math.abs(pct));
  return (
    <div
      className="rounded border border-border/60 bg-muted/10 px-3 py-2 space-y-1"
      title="Total token-usage delta across all flows, as % of base probe run's total tokens. Direct proxy for per-user API billing impact."
    >
      <div className="flex items-baseline justify-between">
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
          tokens · API cost impact
        </span>
        <span className="text-[12px] font-mono tabular-nums text-foreground">
          {formatSigned(delta)}
          <span className="ml-2 text-muted-foreground">
            {pct === null ? "no baseline" : formatPct(pct)}
          </span>
        </span>
      </div>
      <div className="h-[2px] rounded-full bg-muted overflow-hidden relative">
        <div
          className="absolute top-0 h-full bg-muted-foreground/40 rounded-full"
          style={{
            left: delta < 0 ? `${50 - width / 2}%` : "50%",
            width: `${width / 2}%`,
          }}
        />
      </div>
    </div>
  );
}

function AxisRow({
  axes,
  baseline,
}: {
  axes: import("@/types/artifact").Axes;
  baseline: import("@/types/artifact").Axes | null;
}) {
  const items: Array<{ key: keyof import("@/types/artifact").Axes; value: number }> = [
    { key: "continuation", value: axes.continuation },
    { key: "runtime", value: axes.runtime },
    { key: "operational", value: axes.operational },
  ];
  return (
    <div className="grid grid-cols-3 gap-3">
      {items.map((it) => {
        const denom = baseline ? baseline[it.key] : null;
        const width = pctOfBaseline(it.value, denom);
        const pctLabel =
          denom && denom > 0
            ? formatPct((it.value / denom) * 100)
            : "—";
        return (
          <div
            key={it.key}
            className="rounded border border-border/60 bg-muted/10 px-3 py-2 space-y-1"
            title={
              denom && denom > 0
                ? `${it.key}: ${formatSigned(it.value)} of ${denom} baseline cost (${pctLabel})`
                : `${it.key}: no baseline denominator yet`
            }
          >
            <div className="flex items-baseline justify-between">
              <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
                {it.key}
              </span>
              <span className="text-[12px] font-mono tabular-nums text-foreground">
                {formatSigned(it.value)}
                <span className="ml-2 text-muted-foreground text-[10px]">
                  {pctLabel}
                </span>
              </span>
            </div>
            <div className="h-[2px] rounded-full bg-muted overflow-hidden relative">
              <div
                className="absolute top-0 h-full bg-muted-foreground/40 rounded-full"
                style={{
                  left: it.value < 0 ? `${50 - width / 2}%` : "50%",
                  width: `${width / 2}%`,
                }}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
}

function FlowContribRow({
  name,
  net,
  denom,
}: {
  name: string;
  net: number;
  denom: number;
}) {
  const width = pctOfBaseline(net, denom);
  const pctLabel = denom > 0 ? formatPct((net / denom) * 100) : null;
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 space-y-1.5">
      <div className="flex items-baseline gap-3">
        <span className="text-[12px] text-foreground">{name}</span>
        <span className="ml-auto text-[11px] font-mono tabular-nums text-foreground">
          {formatSigned(net)}
          {pctLabel && (
            <span className="ml-2 text-muted-foreground">{pctLabel}</span>
          )}
        </span>
      </div>
      <div className="h-[3px] rounded-full bg-muted overflow-hidden relative">
        <div
          className="absolute top-0 h-full bg-muted-foreground/40 rounded-full"
          style={{
            left: net < 0 ? `${50 - width / 2}%` : "50%",
            width: `${width / 2}%`,
          }}
        />
      </div>
    </div>
  );
}

function formatSigned(n: number): string {
  if (n === 0) return "0";
  if (n > 0) return `+${n}`;
  return `\u2212${Math.abs(n)}`;
}

/**
 * PR-level Intent & Proof aggregate. Lists per-flow verdicts as a
 * clickable index — click a row to open that flow's full Intent & Proof
 * tab. The intent itself is rendered once at the top (same data every
 * flow shares).
 */
function PrProof({
  artifact,
  onTop,
}: {
  artifact: Artifact;
  onTop: (t: TopTab) => void;
}) {
  const status = artifact.proof_status ?? "not-run";
  const flows = artifact.flows ?? [];
  const hasIntent = artifact.intent != null;

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">
          Intent &amp; Proof · analysing
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Matching every flow to the PR's stated intent and hunting for
          evidence. Per-flow verdicts land here as they complete.
        </p>
      </div>
    );
  }

  if (status === "not-run") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">
          Intent & Proof
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          {hasIntent
            ? "Not run — this deployment isn't configured with a backend for intent-fit or proof verification."
            : "No intent supplied. Pass intent via the API or CLI to enable the intent + proof passes."}
        </p>
      </div>
    );
  }

  return (
    <section className="space-y-5">
      {artifact.intent && (
        <PrIntentSummary
          intent={artifact.intent}
          summary={artifact.intent_summary ?? null}
        />
      )}
      {status === "errored" && (
        <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 text-[11px] font-mono text-muted-foreground">
          Proof pass reported errors on at least one flow — partial claims may
          be shown.
        </div>
      )}
      <section className="space-y-3">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Per-flow verdicts ({flows.length})
        </h2>
        <ul className="space-y-2">
          {flows.map((f) => (
            <li key={f.id}>
              <PerFlowVerdictRow
                flow={f}
                onOpen={() => onTop({ kind: "flow", flowId: f.id })}
              />
            </li>
          ))}
        </ul>
      </section>
    </section>
  );
}

/** Small colored chip for a fit / proof verdict. Subtle palette —
 *  good = emerald, partial = amber, missing/unrelated = rose,
 *  no-intent = neutral gray. See `@/lib/verdict-color`. */
function VerdictPill({
  label,
  value,
  kind,
}: {
  label: string;
  value: string | undefined;
  kind: "fit" | "proof";
}) {
  const cls =
    kind === "fit"
      ? intentFitPillClass(value as import("@/types/artifact").IntentFitVerdict | undefined)
      : proofPillClass(value as import("@/types/artifact").ProofVerdict | undefined);
  return (
    <span className={"inline-flex items-baseline gap-1 px-1.5 py-0.5 rounded-full border " + cls}>
      <span className="opacity-70">{label}</span>
      <span>{value ?? "—"}</span>
    </span>
  );
}

/** One-row-per-flow card for the PR Proof tab. Collapsed by default —
 *  the GLM rationale can run 600+ words per flow, which drowns the
 *  other rows. Click "Expand" to read it all inline, or click the
 *  flow name / "Open flow" to drill into the FlowWorkspace. */
function PerFlowVerdictRow({ flow, onOpen }: { flow: Flow; onOpen: () => void }) {
  const [expanded, setExpanded] = useState(false);
  const rationale = flow.proof?.reasoning ?? flow.intent_fit?.reasoning ?? "";
  const hasRationale = rationale.trim().length > 0;
  return (
    <div className="rounded border border-border/60 bg-muted/20 hover:bg-muted/30 px-3 py-2.5 space-y-1.5 transition-colors">
      <div className="flex items-baseline gap-3">
        <button
          onClick={onOpen}
          className="text-[12px] font-mono text-foreground hover:underline underline-offset-2"
          title="Open flow"
        >
          {flow.name}
        </button>
        <span className="ml-auto flex items-baseline gap-1.5 text-[10px] font-mono uppercase tracking-wide">
          <VerdictPill label="fit" value={flow.intent_fit?.verdict} kind="fit" />
          <VerdictPill label="proof" value={flow.proof?.verdict} kind="proof" />
        </span>
      </div>
      {hasRationale && (
        <>
          <p
            className={
              "text-[11px] text-muted-foreground leading-relaxed whitespace-pre-wrap " +
              (expanded ? "" : "line-clamp-2")
            }
          >
            {rationale}
          </p>
          <div className="flex items-center gap-3 pt-0.5">
            <button
              onClick={() => setExpanded((v) => !v)}
              className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
            >
              {expanded ? "collapse" : "expand"}
            </button>
            <button
              onClick={onOpen}
              className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
            >
              open flow →
            </button>
          </div>
        </>
      )}
    </div>
  );
}

function PrIntentSummary({
  intent,
  summary,
}: {
  intent: import("@/types/artifact").IntentInput;
  summary: string | null;
}) {
  const [showOriginal, setShowOriginal] = useState(false);
  const isStructured = typeof intent !== "string";
  const rawText = !isStructured ? intent : null;
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2">
      <div className="flex items-baseline gap-2">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Intent
        </h2>
        <span className="text-[10px] font-mono text-muted-foreground">
          {isStructured ? "structured" : summary ? "summarised" : "raw text"}
        </span>
      </div>
      {isStructured ? (
        <div className="space-y-1.5">
          <p className="text-[13px] font-mono font-semibold text-foreground">
            {intent.title}
          </p>
          {intent.summary && (
            <p className="text-[12px] text-muted-foreground leading-relaxed">
              {intent.summary}
            </p>
          )}
          {intent.claims && intent.claims.length > 0 && (
            <p className="text-[10px] font-mono text-muted-foreground">
              {intent.claims.length} claim{intent.claims.length === 1 ? "" : "s"}
            </p>
          )}
        </div>
      ) : summary ? (
        <div className="space-y-2">
          <p className="text-[13px] text-foreground leading-relaxed">{summary}</p>
          {rawText && (
            <>
              <button
                onClick={() => setShowOriginal((v) => !v)}
                className="text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors"
              >
                {showOriginal ? "hide original" : "show original PR description"}
              </button>
              {showOriginal && (
                <pre className="text-[11px] text-muted-foreground whitespace-pre-wrap font-sans max-h-40 overflow-y-auto rounded border border-border/40 bg-background/40 px-2 py-1.5">
                  {rawText}
                </pre>
              )}
            </>
          )}
        </div>
      ) : (
        <pre className="text-[12px] text-foreground whitespace-pre-wrap font-sans max-h-40 overflow-y-auto">
          {intent}
        </pre>
      )}
    </section>
  );
}
