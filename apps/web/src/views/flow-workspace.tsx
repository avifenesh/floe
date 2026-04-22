import { useMemo } from "react";
import type { Artifact, Flow } from "@/types/artifact";
import type { FlowSubTab } from "./types";
import { PrHunks } from "./pr/PrHunks";
import { useState } from "react";
import { flowLabel } from "@/lib/flow-color";
import { filesOfHunk } from "@/lib/artifact";
import { intentFitPillClass, proofPillClass, signedCostTextClass } from "@/lib/verdict-color";
import { costConfidence, CONFIDENCE_THRESHOLD } from "@/lib/cost-confidence";
import type { IntentFitVerdict, ProofVerdict } from "@/types/artifact";
import { NodeDetailPanel } from "./node-detail-panel";
import { SlideSwitch } from "@/components/SlideSwitch";
import { FLOW_SUB_TABS } from "./types";
import { SourceView } from "./source";

interface Props {
  artifact: Artifact;
  jobId: string;
  flow: Flow;
  sub: FlowSubTab;
}

/**
 * Flow workspace. Each flow has its own set of sub-tabs; we render one at
 * a time based on the current sub selection. Every sub-tab is fully
 * implemented: Overview (header + hunks), Flow (graph visualization),
 * Morph (intent vs. result), Delta (cost drivers + evidence + proof
 * ordered by impact), Evidence (claim list), Source (flow-scoped diff),
 * Cost (axes + drivers + proof peer), Intent & Proof (verdict cards).
 */
export function FlowWorkspace({ artifact, jobId, flow, sub }: Props) {
  const order = FLOW_SUB_TABS.findIndex((t) => t.key === sub);
  const body = (() => {
    switch (sub) {
      case "overview":
        return <FlowOverview artifact={artifact} flow={flow} />;
      case "flow":
        return <FlowGraph artifact={artifact} flow={flow} jobId={jobId} />;
      case "morph":
        return <FlowMorph artifact={artifact} flow={flow} />;
      case "delta":
        return <FlowDelta artifact={artifact} flow={flow} />;
      case "evidence":
        return <FlowEvidence flow={flow} />;
      case "source":
        return <FlowSource artifact={artifact} jobId={jobId} flow={flow} />;
      case "cost":
        return <FlowCost artifact={artifact} flow={flow} />;
      case "proof":
        return <FlowProof artifact={artifact} flow={flow} />;
    }
  })();
  return (
    <SlideSwitch viewKey={`flow-${flow.id}-${sub}`} order={order}>
      {body}
    </SlideSwitch>
  );
}

function FlowOverview({ artifact, flow }: { artifact: Artifact; flow: Flow }) {
  const flowHunks = artifact.hunks.filter((h) => flow.hunk_ids.includes(h.id));
  const scoped: Artifact = { ...artifact, hunks: flowHunks };
  const label = flowLabel(flow);
  return (
    <div className="space-y-5">
      <header className="space-y-1.5 rounded-r px-4 py-3 border border-l-[3px] border-border/60 border-l-muted-foreground/30 bg-muted/20">
        <h1 className="text-[15px] font-mono font-semibold text-foreground">
          {label}
        </h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          {flow.rationale}
        </p>
        <div className="flex items-baseline gap-4 pt-1 text-[11px] font-mono text-muted-foreground">
          <span>
            <span className="text-foreground font-semibold tabular-nums">
              {flow.hunk_ids.length}
            </span>{" "}
            hunk{flow.hunk_ids.length === 1 ? "" : "s"}
          </span>
          <span>
            <span className="text-foreground font-semibold tabular-nums">
              {flow.entities.length}
            </span>{" "}
            entit{flow.entities.length === 1 ? "y" : "ies"}
          </span>
        </div>
      </header>
      <section className="space-y-4">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
          Hunks in this flow
        </h2>
        <PrHunks artifact={scoped} />
      </section>
    </div>
  );
}


function FlowSource({
  artifact,
  jobId,
  flow,
}: {
  artifact: Artifact;
  jobId: string;
  flow: Flow;
}) {
  // Collect the set of files this flow's hunks touch on either side of
  // the diff. The sidebar inside SourceView filters its list to just
  // these paths; the rest of the component is reused unchanged.
  const scope = useMemo(() => {
    const files = new Set<string>();
    for (const hid of flow.hunk_ids) {
      for (const path of filesOfHunk(artifact, hid)) {
        files.add(path);
      }
    }
    return { files };
  }, [artifact, flow.hunk_ids]);

  if (scope.files.size === 0) {
    return (
      <div className="text-[12px] text-muted-foreground">
        No files in this flow. That usually means all the flow's hunks resolved
        to entities with no file provenance — an artifact bug, worth reporting.
      </div>
    );
  }
  return <SourceView artifact={artifact} jobId={jobId} scope={scope} />;
}

function FlowEvidence({ flow }: { flow: Flow }) {
  const claims = flow.evidence ?? [];
  if (claims.length === 0) {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Evidence</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          No claims for this flow. Either the flow's hunks don't overlap with
          any active collector, or the evidence pass hasn't run — rebuild the
          artifact to populate.
        </p>
      </div>
    );
  }
  return (
    <section className="space-y-4">
      <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
        Evidence ({claims.length})
      </h2>
      <ol className="space-y-2">
        {claims.map((c) => (
          <li key={c.id}>
            <ClaimRow claim={c} />
          </li>
        ))}
      </ol>
    </section>
  );
}

function ClaimRow({ claim }: { claim: import("@/types/artifact").Claim }) {
  const kindLabel = kindToLabel(claim.kind);
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2.5 space-y-1.5">
      <div className="flex items-baseline gap-2">
        <StrengthPips strength={claim.strength} />
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
          {kindLabel}
        </span>
        <span
          className="ml-auto text-[10px] font-mono text-muted-foreground"
          title={`source: ${claim.provenance.source}@${claim.provenance.version}`}
        >
          {claim.provenance.source.replace(/^adr-evidence\//, "")}
        </span>
      </div>
      <p className="text-[12px] text-foreground leading-relaxed">{claim.text}</p>
      {claim.entities && claim.entities.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-0.5">
          {claim.entities.slice(0, 6).map((e) => (
            <span
              key={e}
              className="text-[10px] font-mono px-1.5 py-0.5 rounded bg-background/60 border border-border/50 text-foreground/80"
            >
              {e}
            </span>
          ))}
          {claim.entities.length > 6 && (
            <span className="text-[10px] font-mono text-muted-foreground self-center">
              +{claim.entities.length - 6} more
            </span>
          )}
        </div>
      )}
    </div>
  );
}

function StrengthPips({ strength }: { strength: "high" | "medium" | "low" }) {
  const count = strength === "high" ? 3 : strength === "medium" ? 2 : 1;
  // Three dots, filled up to count. Single neutral tone — palette rule.
  return (
    <span
      className="inline-flex items-center gap-[3px]"
      title={`strength: ${strength}`}
    >
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          aria-hidden
          className={
            i < count
              ? "inline-block w-1.5 h-1.5 rounded-full bg-foreground/70"
              : "inline-block w-1.5 h-1.5 rounded-full border border-muted-foreground/40"
          }
        />
      ))}
    </span>
  );
}

function kindToLabel(k: import("@/types/artifact").ClaimKind): string {
  switch (k) {
    case "signature-consistency":
      return "signature";
    case "call-chain":
      return "call chain";
    case "cross-file":
      return "cross-file";
    case "single-file":
      return "single file";
    case "test-coverage":
      return "tests";
    case "intent-fit":
      return "intent-fit";
    case "proof":
      return "proof";
    case "observation":
      return "note";
  }
}

function FlowCost({
  artifact,
  flow,
}: {
  artifact: Artifact;
  flow: Flow;
}) {
  const status = artifact.cost_status ?? "not-run";
  const cost = flow.cost;

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost · analysing</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Probing the repo with the pinned navigation model. Base + head
          snapshots run in separate sessions; cost deltas land here when
          both complete. Keep working in other tabs — this fills in when
          ready.
        </p>
      </div>
    );
  }

  if (status === "not-run") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Cost unavailable — the probe pass isn't configured. Set
          <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">ADR_PROBE_LLM</code>
          (or rely on <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">ADR_LLM</code>)
          and re-run the analysis.
        </p>
      </div>
    );
  }

  if (status === "errored") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost · errored</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Probe pass failed. Per-flow cost may be missing or partial.
          Server logs hold the specific failure.
        </p>
      </div>
    );
  }

  if (!cost) {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          No cost estimate for this flow (no matching probe entities).
        </p>
      </div>
    );
  }

  const drivers = cost.drivers ?? [];
  const baseline = artifact.baseline ?? null;
  const baselineAxesSum = baseline
    ? baseline.axes_base.continuation +
      baseline.axes_base.runtime +
      baseline.axes_base.operational
    : 0;
  const netPct = baselineAxesSum > 0 ? (cost.net / baselineAxesSum) * 100 : null;
  const confidence = costConfidence(cost);
  const lowConfidence = confidence < CONFIDENCE_THRESHOLD;
  const heroColor = lowConfidence
    ? "text-muted-foreground"
    : signedCostTextClass(netPct ?? cost.net);
  const tokensDelta = cost.tokens_delta ?? 0;
  const tokensPct = baseline && baseline.tokens_base > 0
    ? (tokensDelta / baseline.tokens_base) * 100
    : null;

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
                <span className={lowConfidence ? "" : signedCostTextClass(cost.net)}>
                  {formatSigned(cost.net)}
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
              {formatSigned(cost.net)}
            </span>
          )}
          <span className="text-[12px] text-muted-foreground">
            navigation-cost delta
          </span>
          {lowConfidence && (
            <span
              title={`Driver confidence ${(confidence * 100).toFixed(0)}% (< ${(CONFIDENCE_THRESHOLD * 100).toFixed(0)}%) — read the drivers below; the headline net is unreliable.`}
              className="text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-amber-400/40 bg-amber-50 dark:bg-amber-400/10 text-amber-800 dark:text-amber-200"
            >
              low confidence
            </span>
          )}
          <span
            className="ml-auto text-[10px] font-mono text-muted-foreground"
            title={`probe model: ${cost.probe_model}@${cost.probe_set_version}`}
          >
            {cost.probe_model}
          </span>
        </div>
        <AxisRow axes={cost.axes} baseline={baseline?.axes_base ?? null} />
        <TokensRow delta={tokensDelta} pct={tokensPct} />
        <FlowBaselineStamp
          probeModel={cost.probe_model}
          probeSetVersion={cost.probe_set_version}
        />
      </header>
      <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
        Drivers ({drivers.length})
      </h2>
      {drivers.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">
          No per-probe delta on the entities in this flow — the probes
          visited these names equally on base and head.
        </p>
      ) : (
        <ol className="space-y-2">
          {drivers.map((d, i) => (
            <li key={i}>
              <CostDriverRow driver={d} baseline={baseline} />
            </li>
          ))}
        </ol>
      )}
    </section>
  );
}

/**
 * Movement as a percentage of the per-repo baseline — Avi's rule.
 * Returns 0 when denominator is 0 (proof axis in v0, or empty baseline).
 * Caps at 100 because we're communicating *movement*, not rank; a
 * delta larger than the baseline still reads as "fully saturated".
 */
function pctOfBaseline(value: number, denom: number | null | undefined): number {
  if (!denom || denom <= 0) return 0;
  const raw = (Math.abs(value) / denom) * 100;
  return Math.min(100, raw);
}

function TokensRow({ delta, pct }: { delta: number; pct: number | null }) {
  const width = pct === null ? 0 : Math.min(100, Math.abs(pct));
  const sign = delta < 0 ? "\u2212" : delta > 0 ? "+" : "";
  const pctLabel =
    pct === null ? "no baseline" : `${sign}${Math.abs(pct).toFixed(1)}%`;
  return (
    <div
      className="rounded border border-border/60 bg-muted/10 px-3 py-2 space-y-1"
      title="Token-usage delta on the flow's entities, as % of the base probe run's total token usage. Direct proxy for per-user API billing impact."
    >
      <div className="flex items-baseline justify-between">
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
          tokens · API cost impact
        </span>
        <span className="text-[12px] font-mono tabular-nums text-foreground">
          {formatSigned(delta)}
          <span className="ml-2 text-muted-foreground">{pctLabel}</span>
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
            ? `${it.value < 0 ? "\u2212" : it.value > 0 ? "+" : ""}${(
                (Math.abs(it.value) / denom) *
                100
              ).toFixed(1)}%`
            : "—";
        return (
          <div
            key={it.key}
            className="rounded border border-border/60 bg-muted/10 px-3 py-2 space-y-1"
            title={
              denom && denom > 0
                ? `${it.key}: ${formatSigned(it.value)} of ${denom} baseline cost (${pctLabel})`
                : `${it.key}: no baseline denominator yet (probe may not map to this axis)`
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
              {/* Centered zero axis: negative fills left, positive fills right.
                  Width is % of the per-repo baseline on this axis, capped 100.
                  Bars read as "how much did nav cost move relative to the
                  repo's existing nav cost" — not relative-rank. */}
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

function CostDriverRow({
  driver,
  baseline,
}: {
  driver: import("@/types/artifact").CostDriver;
  baseline: import("@/types/artifact").ArtifactBaseline | null;
}) {
  const denom = driverDenominator(driver, baseline);
  const width = pctOfBaseline(driver.value, denom);
  const pctLabel =
    denom && denom > 0
      ? `${driver.value < 0 ? "\u2212" : driver.value > 0 ? "+" : ""}${(
          (Math.abs(driver.value) / denom) *
          100
        ).toFixed(1)}%`
      : null;
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 space-y-1.5">
      <div className="flex items-baseline gap-3">
        <span className="text-[12px] text-foreground">{driver.label}</span>
        <span className="ml-auto text-[11px] font-mono tabular-nums text-foreground">
          {formatSigned(driver.value)}
          {pctLabel && (
            <span className="ml-2 text-muted-foreground">{pctLabel}</span>
          )}
        </span>
      </div>
      <div className="h-[3px] rounded-full bg-muted overflow-hidden relative">
        <div
          className="absolute top-0 h-full rounded-full bg-muted-foreground/40"
          style={{
            left: driver.value < 0 ? `${50 - width / 2}%` : "50%",
            width: `${width / 2}%`,
          }}
        />
      </div>
      {driver.detail && (
        <p className="text-[11px] text-muted-foreground leading-relaxed">
          {driver.detail}
        </p>
      )}
    </div>
  );
}

/**
 * Driver labels come from `adr-cost` — "API-surface navigation",
 * "external-boundary reach", "type call-site tracing" — each corresponds
 * to one probe and therefore one axis. Map label → baseline denominator
 * so the per-driver bar scales the same way the aggregate axis does.
 */
function driverDenominator(
  driver: import("@/types/artifact").CostDriver,
  baseline: import("@/types/artifact").ArtifactBaseline | null,
): number | null {
  if (!baseline) return null;
  const l = driver.label.toLowerCase();
  if (l.includes("api")) return baseline.axes_base.continuation;
  if (l.includes("external")) return baseline.axes_base.operational;
  if (l.includes("type")) return baseline.axes_base.runtime;
  return null;
}

function formatSigned(n: number): string {
  if (n === 0) return "0";
  if (n > 0) return `+${n}`;
  // Use a real unicode minus for typographic consistency with the rest
  // of the UI (avoids the hyphen-like ASCII dash).
  return `\u2212${Math.abs(n)}`;
}

function FlowBaselineStamp({
  probeModel,
  probeSetVersion,
}: {
  probeModel: string;
  probeSetVersion: string;
}) {
  return (
    <div
      className="flex items-baseline gap-2 text-[10px] font-mono text-muted-foreground"
      title="Baseline pin for this flow's cost. Shared with PR-aggregate when both ran in the same probe pass."
    >
      <span className="uppercase tracking-wide opacity-70">pinned to</span>
      <span className="text-foreground/80">{probeModel}</span>
      <span className="opacity-50">·</span>
      <span>probes {probeSetVersion}</span>
    </div>
  );
}

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

/** Per-flow Morph view (RFC view 03) — intent-vs-result.
 *
 *  Reads the PR's structured intent (or the LLM-summarised raw text)
 *  and the flow's intent-fit + proof results, then renders the
 *  *match*: each intent claim → which flow entities deliver it,
 *  with the proof-pass verdict beside it. Plus a replacements panel:
 *  base entities removed in this flow paired with head entities
 *  added in the same file (heuristic refactor pairs the LLM rarely
 *  surfaces explicitly).
 *
 *  This is the differentiator: nothing else in PR review tells you
 *  "the PR claimed X, this flow delivers it via these specific
 *  symbols, and here's the evidence the LLM found." */
function FlowMorph({ artifact, flow }: { artifact: Artifact; flow: Flow }) {
  const intent = artifact.intent;
  const intentClaims = intent && typeof intent !== "string" ? intent.claims ?? [] : [];
  const intentSummary =
    artifact.intent_summary ??
    (intent && typeof intent === "string" ? intent : null);
  const fit = flow.intent_fit ?? null;
  const proofClaims = flow.proof?.claims ?? [];

  // Resolve every entity in the flow against base + head graphs so
  // we can paint claim deliverers with status color + detect
  // refactor pairs.
  const entityStatus = new Map<string, MorphStatus>();
  for (const name of flow.entities ?? []) {
    const base = findNodeByName(artifact.base.nodes, name);
    const head = findNodeByName(artifact.head.nodes, name);
    entityStatus.set(
      name,
      !base && head
        ? "added"
        : base && !head
          ? "removed"
          : base && head && nodeSignature(base) !== nodeSignature(head)
            ? "changed"
            : "unchanged",
    );
  }

  // Replacement detection: base-only entities + head-only entities
  // sharing a file are likely refactor pairs (rename / extract).
  const removed = [...entityStatus.entries()].filter(([, s]) => s === "removed");
  const added = [...entityStatus.entries()].filter(([, s]) => s === "added");
  const replacements: { from: string; to: string; file: string }[] = [];
  const usedAdded = new Set<string>();
  for (const [rname] of removed) {
    const rNode = findNodeByName(artifact.base.nodes, rname);
    if (!rNode) continue;
    for (const [aname] of added) {
      if (usedAdded.has(aname)) continue;
      const aNode = findNodeByName(artifact.head.nodes, aname);
      if (!aNode || aNode.file !== rNode.file) continue;
      replacements.push({ from: rname, to: aname, file: aNode.file });
      usedAdded.add(aname);
      break;
    }
  }

  // Pair each intent claim with the proof status the LLM emitted for
  // it (matches by claim_index when available, otherwise by order).
  const claimPanels: ClaimPanelData[] = (intentClaims.length > 0 ? intentClaims : []).map((c, idx) => {
    const proof = proofClaims.find((p) => p.claim_index === idx) ?? proofClaims[idx] ?? null;
    const matched = fit?.matched_claims?.includes(idx) ?? false;
    return { idx, text: c.statement, evidenceType: c.evidence_type, proof, matched };
  });

  // When intent is raw text we don't have indexed claims — fall back
  // to surfacing whatever proof claims came back.
  const fallbackProofPanels: ClaimPanelData[] =
    intentClaims.length === 0
      ? proofClaims.map((p, idx) => ({
          idx,
          text: p.statement,
          evidenceType: "observation",
          proof: p,
          matched: false,
        }))
      : [];
  const panels = [...claimPanels, ...fallbackProofPanels];

  return (
    <div className="space-y-5">
      <header className="space-y-1">
        <h1 className="text-[15px] font-mono text-foreground">
          Morph
          <span className="font-normal text-muted-foreground"> · intent vs result for {flow.name}</span>
        </h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          What the PR claimed, what this flow actually delivers, and
          which symbols carry the claim. Refactor pairs surface below.
        </p>
        {intentSummary && (
          <p className="text-[12px] text-foreground leading-relaxed pt-1 italic">
            "{intentSummary}"
          </p>
        )}
      </header>

      {/* Top-line per-flow verdicts */}
      <section className="flex items-center gap-3 flex-wrap text-[10px] font-mono">
        <span className="uppercase tracking-wide text-muted-foreground">flow verdict</span>
        <span className={"px-2 py-0.5 rounded-full border " + intentFitPillClass(fit?.verdict)}>
          fit · {fit?.verdict ?? "—"}
        </span>
        <span className={"px-2 py-0.5 rounded-full border " + proofPillClass(flow.proof?.verdict)}>
          proof · {flow.proof?.verdict ?? "—"}
        </span>
        {fit?.reasoning && (
          <span className="text-[11px] text-muted-foreground italic flex-1 min-w-0 truncate" title={fit.reasoning}>
            {fit.reasoning}
          </span>
        )}
      </section>

      {/* Claim → entities mapping */}
      {panels.length > 0 && (
        <section className="space-y-2">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Claim ↔ deliverers ({panels.length})
          </h2>
          <ul className="space-y-2">
            {panels.map((p) => (
              <li key={`claim-${p.idx}`}>
                <ClaimPanel
                  index={p.idx}
                  text={p.text}
                  evidenceType={p.evidenceType}
                  proof={p.proof}
                  matched={p.matched}
                  flowEntities={flow.entities ?? []}
                  entityStatus={entityStatus}
                />
              </li>
            ))}
          </ul>
        </section>
      )}

      {!intent && (
        <p className="text-[12px] text-muted-foreground">
          No intent supplied — pass a PR description or structured intent to see the claim-match panel.
        </p>
      )}

      {/* Replacement pairs */}
      {replacements.length > 0 && (
        <section className="space-y-2">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Replacements ({replacements.length})
          </h2>
          <ul className="space-y-1.5">
            {replacements.map((r) => (
              <li key={`${r.from}->${r.to}`}>
                <ReplacementRow from={r.from} to={r.to} file={r.file} />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

const STATUS_DOT: Record<MorphStatus, string> = {
  added: "bg-emerald-500/80",
  removed: "bg-rose-500/80",
  changed: "bg-amber-500/80",
  unchanged: "bg-muted-foreground/40",
};

interface ClaimPanelData {
  idx: number;
  text: string;
  evidenceType: string;
  proof: import("@/types/artifact").ClaimProofStatus | null;
  matched: boolean;
}

function ClaimPanel({
  index,
  text,
  evidenceType,
  proof,
  matched,
  flowEntities,
  entityStatus,
}: {
  index: number;
  text: string;
  evidenceType: string;
  proof: import("@/types/artifact").ClaimProofStatus | null;
  matched: boolean;
  flowEntities: string[];
  entityStatus: Map<string, MorphStatus>;
}) {
  const status = proof?.status;
  const tone =
    status === "found" ? "good" : status === "partial" ? "partial" : status === "missing" ? "bad" : "neutral";
  const evidence = proof?.evidence ?? [];
  // Heuristic: any flow entity name that appears in any evidence
  // detail / path is a "deliverer" of this claim.
  const deliverers = flowEntities.filter((name) =>
    evidence.some((e) => (e.detail ?? "").includes(name) || (e.path ?? "").includes(name)),
  );
  return (
    <div className="rounded border border-border/60 border-l-[3px] bg-muted/10 px-3 py-2.5 space-y-2"
         style={{ borderLeftColor: tone === "good" ? "rgb(16 185 129 / 0.7)" : tone === "partial" ? "rgb(245 158 11 / 0.7)" : tone === "bad" ? "rgb(244 63 94 / 0.7)" : undefined }}>
      <div className="flex items-baseline gap-2">
        <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wide">
          claim #{index}
        </span>
        <span className="text-[10px] font-mono text-muted-foreground">
          {evidenceType}
        </span>
        {matched && (
          <span className="text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200">
            matched by fit
          </span>
        )}
        {status && (
          <span
            className={
              "ml-auto text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border " +
              (status === "found"
                ? "border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200"
                : status === "partial"
                  ? "border-amber-400/40 bg-amber-50 text-amber-800 dark:bg-amber-400/10 dark:text-amber-200"
                  : "border-rose-400/40 bg-rose-50 text-rose-800 dark:bg-rose-400/10 dark:text-rose-200")
            }
          >
            proof · {status}
          </span>
        )}
      </div>
      <p className="text-[12px] text-foreground leading-snug">{text}</p>
      {deliverers.length > 0 && (
        <div className="flex items-baseline gap-2 flex-wrap pt-1">
          <span className="text-[10px] font-mono text-muted-foreground uppercase tracking-wide">
            via
          </span>
          {deliverers.map((name) => {
            const s = entityStatus.get(name) ?? "unchanged";
            return (
              <span
                key={name}
                className="inline-flex items-center gap-1.5 text-[11px] font-mono text-foreground rounded px-1.5 py-0.5 bg-background/60 border border-border/60"
                title={`${name} · ${s}`}
              >
                <span className={"w-1.5 h-1.5 rounded-full " + STATUS_DOT[s]} aria-hidden />
                {name}
              </span>
            );
          })}
        </div>
      )}
      {evidence.length > 0 && (
        <ul className="text-[11px] text-muted-foreground leading-snug space-y-0.5 pt-1">
          {evidence.slice(0, 3).map((e, i) => (
            <li key={i} className="truncate" title={e.detail}>
              <span className="text-muted-foreground/70 mr-1">·</span>
              {e.detail}
              {e.path && <span className="text-muted-foreground/70 ml-2 text-[10px] font-mono">{e.path}</span>}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function ReplacementRow({ from, to, file }: { from: string; to: string; file: string }) {
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 flex items-baseline gap-2 flex-wrap">
      <span className="inline-flex items-center gap-1.5 text-[12px] font-mono text-foreground">
        <span className="w-1.5 h-1.5 rounded-full bg-rose-500/80" aria-hidden />
        {from}
      </span>
      <span className="text-muted-foreground">→</span>
      <span className="inline-flex items-center gap-1.5 text-[12px] font-mono text-foreground">
        <span className="w-1.5 h-1.5 rounded-full bg-emerald-500/80" aria-hidden />
        {to}
      </span>
      <span className="ml-auto text-[10px] font-mono text-muted-foreground truncate">
        {file}
      </span>
    </div>
  );
}

/** Per-flow Flow view (RFC view 02) — runtime trajectory through
 *  the *code*, not just between functions. Each entity in the flow
 *  becomes a column showing its real CFG (entry → seq → branch →
 *  loop → return / async / throw); call edges from the head graph
 *  weave between columns from the calling block to the callee's
 *  entry. Node SHAPE encodes CFG kind, COLOR encodes morph status
 *  (added / removed / changed). This is "flow in code" — the
 *  reviewer follows the runtime trajectory the way the program
 *  actually executes, not an abstract DAG. */
function FlowGraph({ artifact, flow, jobId }: { artifact: Artifact; flow: Flow; jobId: string }) {
  const [selected, setSelected] = useState<string | null>(null);
  const [side, setSide] = useState<"head" | "base">("head");
  // Scope toggle (RFC: "All flows stacked (or N overlays, reviewer
  // switches)"). `this` renders the current flow's columns; `all`
  // renders every flow in the artifact concatenated with separators.
  const [scope, setScope] = useState<"this" | "all">("this");
  // Collect entities for the active scope. De-dup across flows so a
  // shared entity only gets one column (with a tooltip listing which
  // flows it participates in).
  const entities: string[] = (() => {
    if (scope === "this") return flow.entities ?? [];
    const seen = new Set<string>();
    const out: string[] = [];
    for (const f of artifact.flows ?? []) {
      for (const e of f.entities ?? []) {
        if (!seen.has(e)) {
          seen.add(e);
          out.push(e);
        }
      }
    }
    return out;
  })();
  const items = entities.map((name) => {
    const base = findNodeByName(artifact.base.nodes, name);
    const head = findNodeByName(artifact.head.nodes, name);
    const status: MorphStatus =
      !base && head
        ? "added"
        : base && !head
          ? "removed"
          : base && head && nodeSignature(base) !== nodeSignature(head)
            ? "changed"
            : "unchanged";
    const headCfg = head ? findCfg(artifact.head_cfg, head.id) : null;
    const baseCfg = base ? findCfg(artifact.base_cfg, base.id) : null;
    // Single-side view picks a canonical CFG for layout; both-mode
    // uses head as the primary and overlays base.
    const cfg =
      side === "base"
        ? baseCfg ?? headCfg
        : headCfg ?? baseCfg;
    return { name, base, head, status, cfg, headCfg, baseCfg };
  });
  // Inter-entity call edges (head graph). Used to link columns.
  const idToName = new Map<number, string>();
  for (const it of items) {
    if (it.head) idToName.set(it.head.id, it.name);
    else if (it.base) idToName.set(it.base.id, it.name);
  }
  const callEdges: { from: string; to: string; side: "head" | "base" }[] = [];
  const seen = new Set<string>();
  for (const e of artifact.head.edges ?? []) {
    const f = idToName.get(e.from);
    const t = idToName.get(e.to);
    if (!f || !t || f === t) continue;
    const k = `${f}->${t}`;
    if (seen.has(k)) continue;
    seen.add(k);
    callEdges.push({ from: f, to: t, side: "head" });
  }
  for (const e of artifact.base.edges ?? []) {
    const f = idToName.get(e.from);
    const t = idToName.get(e.to);
    if (!f || !t || f === t) continue;
    const k = `${f}->${t}`;
    if (seen.has(k)) continue;
    seen.add(k);
    callEdges.push({ from: f, to: t, side: "base" });
  }
  const counts = items.reduce(
    (acc, r) => ({ ...acc, [r.status]: (acc[r.status] ?? 0) + 1 }),
    { added: 0, removed: 0, changed: 0, unchanged: 0 } as Record<string, number>,
  );

  return (
    <div className="space-y-4">
      <header className="space-y-1">
        <h1 className="text-[15px] font-mono text-foreground">
          Flow
          <span className="font-normal text-muted-foreground"> · runtime trajectory of {flow.name}</span>
        </h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Each column is an entity's control-flow graph — entry, branches,
          loops, async boundaries, returns. Curved arrows are call edges
          between entities. Column header color marks base → head movement.
        </p>
        <div className="flex items-center gap-3 text-[10px] font-mono text-muted-foreground">
          <MorphLegend status="added" count={counts.added} />
          <MorphLegend status="changed" count={counts.changed} />
          <MorphLegend status="removed" count={counts.removed} />
          <MorphLegend status="unchanged" count={counts.unchanged} />
          <div className="ml-auto inline-flex items-center gap-2">
            <div className="inline-flex items-center gap-0.5 rounded-md border border-border/60 p-0.5">
              {(["this", "all"] as const).map((s) => (
                <button
                  key={s}
                  onClick={() => setScope(s)}
                  className={
                    "px-2 py-0.5 rounded-sm transition-colors " +
                    (scope === s
                      ? "bg-foreground/90 text-background"
                      : "text-muted-foreground hover:text-foreground")
                  }
                >
                  {s === "this" ? "this flow" : "all flows"}
                </button>
              ))}
            </div>
            <div className="inline-flex items-center gap-0.5 rounded-md border border-border/60 p-0.5">
              {(["base", "head"] as const).map((s) => (
                <button
                  key={s}
                  onClick={() => setSide(s)}
                  className={
                    "px-2 py-0.5 rounded-sm transition-colors " +
                    (side === s
                      ? "bg-foreground/90 text-background"
                      : "text-muted-foreground hover:text-foreground")
                  }
                >
                  {s}
                </button>
              ))}
            </div>
          </div>
        </div>
      </header>

      {items.length === 0 ? (
        <p className="text-[12px] text-muted-foreground">
          No entities resolved on this flow.
        </p>
      ) : (
        <>
          <PropagationStrip flow={flow} />
          <CfgFlowDiagram
            items={items}
            callEdges={callEdges}
            onSelect={setSelected}
          />
          {selected && (
            <NodeDetailPanel
              artifact={artifact}
              jobId={jobId}
              entity={selected}
              onClose={() => setSelected(null)}
            />
          )}
        </>
      )}
    </div>
  );
}

/** Strip of external callers/callees that reach this flow's entities.
 *  Populated from `Flow.propagation_edges` (1-hop in v0). Split into
 *  "→ in" (external entity → flow entity) and "out →" (flow entity
 *  → external) columns so the reviewer sees both directions. */
function PropagationStrip({ flow }: { flow: Flow }) {
  const edges = flow.propagation_edges ?? [];
  if (edges.length === 0) return null;
  const entitySet = new Set(flow.entities ?? []);
  const incoming: { external: string; into: string }[] = [];
  const outgoing: { from: string; external: string }[] = [];
  for (const [from, to] of edges) {
    const fromIn = entitySet.has(from);
    const toIn = entitySet.has(to);
    if (!fromIn && toIn) incoming.push({ external: from, into: to });
    else if (fromIn && !toIn) outgoing.push({ from, external: to });
  }
  if (incoming.length === 0 && outgoing.length === 0) return null;
  return (
    <div className="rounded border border-border/60 bg-muted/5 px-3 py-2 text-[11px] font-mono space-y-1.5">
      {incoming.length > 0 && (
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className="text-muted-foreground uppercase tracking-wide text-[10px]">
            calls in ({incoming.length})
          </span>
          {incoming.map((p, i) => (
            <span
              key={`in-${i}`}
              className="inline-flex items-baseline gap-1 rounded border border-dashed border-border/60 bg-background/60 px-1.5 py-0.5 text-foreground"
              title={`${p.external} → ${p.into}`}
            >
              {p.external}
              <span className="text-muted-foreground/70">→</span>
              <span className="text-muted-foreground">{p.into}</span>
            </span>
          ))}
        </div>
      )}
      {outgoing.length > 0 && (
        <div className="flex items-baseline gap-2 flex-wrap">
          <span className="text-muted-foreground uppercase tracking-wide text-[10px]">
            calls out ({outgoing.length})
          </span>
          {outgoing.map((p, i) => (
            <span
              key={`out-${i}`}
              className="inline-flex items-baseline gap-1 rounded border border-dashed border-border/60 bg-background/60 px-1.5 py-0.5 text-foreground"
              title={`${p.from} → ${p.external}`}
            >
              <span className="text-muted-foreground">{p.from}</span>
              <span className="text-muted-foreground/70">→</span>
              {p.external}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

function findCfg(
  entries: import("@/types/artifact").CfgEntry[] | undefined,
  fnId: number | undefined,
): import("@/types/artifact").Cfg | null {
  if (!entries || fnId === undefined) return null;
  const hit = entries.find((e) => e.function === fnId);
  return hit?.cfg ?? null;
}

type MorphStatus = "added" | "removed" | "changed" | "unchanged";

const MORPH_NODE_BG: Record<MorphStatus, string> = {
  added: "fill-emerald-500/15 stroke-emerald-500/70",
  removed: "fill-rose-500/15 stroke-rose-500/70",
  changed: "fill-amber-500/15 stroke-amber-500/70",
  unchanged: "fill-muted/30 stroke-border",
};

const MORPH_LEGEND_DOT: Record<MorphStatus, string> = {
  added: "bg-emerald-500/70",
  removed: "bg-rose-500/70",
  changed: "bg-amber-500/70",
  unchanged: "bg-muted-foreground/40",
};

function MorphLegend({ status, count }: { status: MorphStatus; count: number }) {
  return (
    <span className="inline-flex items-center gap-1.5">
      <span className={"w-2 h-2 rounded-full " + MORPH_LEGEND_DOT[status]} aria-hidden />
      {count} {status}
    </span>
  );
}

const PAD = 16;

interface CfgItem {
  name: string;
  base: import("@/types/artifact").Node | null;
  head: import("@/types/artifact").Node | null;
  status: MorphStatus;
  /** Canonical CFG used for layout — head by default, base when the
   *  side toggle is on "base". Overlay of the other side happens on top. */
  cfg: import("@/types/artifact").Cfg | null;
  headCfg: import("@/types/artifact").Cfg | null;
  baseCfg: import("@/types/artifact").Cfg | null;
}

const COL_W = 200;
const HEADER_H = 36;
const CFG_NODE_W = 140;
const CFG_NODE_H = 28;
const CFG_GAP_Y = 14;
const CFG_GAP_X = 40;
const CFG_PAD_TOP = 12;
const CFG_NODE_X_OFFSET = (COL_W - CFG_NODE_W) / 2;

const CFG_KIND_LABEL: Record<string, string> = {
  entry: "entry",
  exit: "exit",
  seq: "stmt",
  branch: "branch",
  loop: "loop",
  "async-boundary": "await",
  throw: "throw",
  try: "try",
  return: "return",
};

const CFG_KIND_FILL: Record<string, string> = {
  entry: "fill-emerald-500/20 stroke-emerald-500/70",
  exit: "fill-muted/50 stroke-border",
  return: "fill-muted/40 stroke-muted-foreground/60",
  seq: "fill-muted/30 stroke-border",
  branch: "fill-amber-500/15 stroke-amber-500/60",
  loop: "fill-amber-500/15 stroke-amber-500/60",
  "async-boundary": "fill-sky-500/15 stroke-sky-500/60",
  throw: "fill-rose-500/20 stroke-rose-500/70",
  try: "fill-sky-500/10 stroke-sky-500/40",
};

interface CfgLayout {
  positions: Map<number, { x: number; y: number }>;
  width: number;
  height: number;
}

/** Lay out a CFG vertically: BFS from entry assigns Y level, same-
 *  level nodes spread X. Cycles fall back to insertion order. Branch
 *  arms naturally land side by side because they share a level. */
function layoutCfg(cfg: import("@/types/artifact").Cfg): CfgLayout {
  const out = new Map<number, { x: number; y: number }>();
  if (!cfg || cfg.nodes.length === 0) {
    return { positions: out, width: COL_W, height: 0 };
  }
  const incoming = new Map<number, Set<number>>();
  const outgoing = new Map<number, Set<number>>();
  for (const n of cfg.nodes) {
    incoming.set(n.id, new Set());
    outgoing.set(n.id, new Set());
  }
  for (const e of cfg.edges) {
    incoming.get(e.to)?.add(e.from);
    outgoing.get(e.from)?.add(e.to);
  }
  const level = new Map<number, number>();
  const queue: number[] = [];
  // Find entry nodes (kind === "entry" preferred; else nodes with no incoming).
  const entries = cfg.nodes.filter((n) => (n.kind as { type: string }).type === "entry");
  const seeds = entries.length > 0 ? entries.map((n) => n.id) : cfg.nodes.filter((n) => (incoming.get(n.id)?.size ?? 0) === 0).map((n) => n.id);
  for (const id of seeds) {
    level.set(id, 0);
    queue.push(id);
  }
  // Cap depth so cycles in the CFG (loop bodies, try/catch back-edges)
  // don't drive the level counter to infinity. Stop relaxing past the
  // node count — that's the longest acyclic path possible.
  const maxAllowedLevel = cfg.nodes.length;
  while (queue.length > 0) {
    const cur = queue.shift()!;
    const lv = level.get(cur)!;
    if (lv >= maxAllowedLevel) continue;
    for (const next of outgoing.get(cur) ?? []) {
      const want = lv + 1;
      if ((level.get(next) ?? -1) < want && want <= maxAllowedLevel) {
        level.set(next, want);
        queue.push(next);
      }
    }
  }
  for (const n of cfg.nodes) if (!level.has(n.id)) level.set(n.id, 0);
  const byLevel = new Map<number, number[]>();
  for (const n of cfg.nodes) {
    const lv = level.get(n.id)!;
    if (!byLevel.has(lv)) byLevel.set(lv, []);
    byLevel.get(lv)!.push(n.id);
  }
  let maxRowWidth = 1;
  let maxLevel = 0;
  for (const [lv, ids] of byLevel) {
    maxRowWidth = Math.max(maxRowWidth, ids.length);
    maxLevel = Math.max(maxLevel, lv);
    ids.forEach((id, i) => {
      const x = CFG_NODE_X_OFFSET + (i - (ids.length - 1) / 2) * (CFG_NODE_W + 8);
      const y = CFG_PAD_TOP + lv * (CFG_NODE_H + CFG_GAP_Y);
      out.set(id, { x, y });
    });
  }
  const colWidth = Math.max(COL_W, CFG_NODE_X_OFFSET + maxRowWidth * (CFG_NODE_W + 8));
  const colHeight = CFG_PAD_TOP + (maxLevel + 1) * (CFG_NODE_H + CFG_GAP_Y);
  return { positions: out, width: colWidth, height: colHeight };
}

function CfgFlowDiagram({
  items,
  callEdges,
  onSelect,
}: {
  items: CfgItem[];
  callEdges: { from: string; to: string; side: "head" | "base" }[];
  onSelect?: (entity: string) => void;
}) {
  // Per-column layout. Compute each column's CFG positions + height.
  const cols = items.map((it) => {
    const layout = it.cfg ? layoutCfg(it.cfg) : { positions: new Map(), width: COL_W, height: 0 };
    return { item: it, layout };
  });
  // Column X positions: cumulative sum of column widths + horizontal gap.
  let runningX = PAD;
  const colX = new Map<string, number>();
  for (const c of cols) {
    colX.set(c.item.name, runningX);
    runningX += c.layout.width + CFG_GAP_X;
  }
  const totalW = runningX - CFG_GAP_X + PAD;
  const maxColH = cols.reduce((m, c) => Math.max(m, c.layout.height), 0);
  const totalH = HEADER_H + maxColH + PAD;

  // Per-column entry/exit Y positions for inter-column arrows.
  const colEntryY = new Map<string, number>();
  const colExitY = new Map<string, number>();
  for (const c of cols) {
    if (!c.item.cfg) continue;
    const entry = c.item.cfg.nodes.find((n) => (n.kind as { type: string }).type === "entry");
    const exit = c.item.cfg.nodes.find((n) => (n.kind as { type: string }).type === "exit");
    if (entry) colEntryY.set(c.item.name, HEADER_H + (c.layout.positions.get(entry.id)?.y ?? 0) + CFG_NODE_H / 2);
    if (exit) colExitY.set(c.item.name, HEADER_H + (c.layout.positions.get(exit.id)?.y ?? 0) + CFG_NODE_H / 2);
  }

  return (
    <div className="overflow-x-auto rounded border border-border/60 bg-muted/10 p-2">
      <svg
        width={totalW}
        height={totalH}
        viewBox={`0 0 ${totalW} ${totalH}`}
        className="block"
      >
        <defs>
          <marker
            id="cfg-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="6"
            markerHeight="6"
            orient="auto"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-muted-foreground/70" />
          </marker>
          <marker
            id="cfg-call-arrow"
            viewBox="0 0 10 10"
            refX="9"
            refY="5"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 5 L 0 10 z" className="fill-sky-500/80" />
          </marker>
        </defs>

        {/* Column headers — click to open NodeDetailPanel */}
        {cols.map((c) => {
          const x = colX.get(c.item.name)!;
          const file = (c.item.head ?? c.item.base)?.file ?? "";
          const fileShort = file.split("/").pop() ?? file;
          const short = c.item.name.length > 28 ? c.item.name.slice(0, 27) + "\u2026" : c.item.name;
          return (
            <g
              key={`hdr-${c.item.name}`}
              onClick={() => onSelect?.(c.item.name)}
              className={onSelect ? "cursor-pointer" : undefined}
            >
              <title>{`${c.item.name}\n${file}\nstatus: ${c.item.status}\nclick for details`}</title>
              <rect
                x={x}
                y={0}
                width={c.layout.width}
                height={HEADER_H - 6}
                rx={6}
                className={MORPH_NODE_BG[c.item.status] + " stroke-[1.2]"}
              />
              <text
                x={x + 10}
                y={14}
                className="fill-foreground"
                style={{ fontFamily: "ui-monospace, monospace", fontSize: 11 }}
              >
                {short}
              </text>
              <text
                x={x + 10}
                y={26}
                className="fill-muted-foreground"
                style={{ fontFamily: "ui-monospace, monospace", fontSize: 9 }}
              >
                {c.item.status} · {fileShort}
              </text>
            </g>
          );
        })}

        {/* CFG edges per column */}
        {cols.flatMap((c) => {
          if (!c.item.cfg) return [];
          const xOff = colX.get(c.item.name)!;
          return c.item.cfg.edges.map((e, i) => {
            const a = c.layout.positions.get(e.from);
            const b = c.layout.positions.get(e.to);
            if (!a || !b) return null;
            const x1 = xOff + a.x + CFG_NODE_W / 2;
            const y1 = HEADER_H + a.y + CFG_NODE_H;
            const x2 = xOff + b.x + CFG_NODE_W / 2;
            const y2 = HEADER_H + b.y;
            return (
              <line
                key={`cfge-${c.item.name}-${i}`}
                x1={x1}
                y1={y1}
                x2={x2}
                y2={y2}
                className="stroke-muted-foreground/50 stroke-[1.2]"
                markerEnd="url(#cfg-arrow)"
              />
            );
          });
        })}

        {/* CFG nodes per column */}
        {cols.flatMap((c) => {
          if (!c.item.cfg) {
            const x = colX.get(c.item.name)!;
            return [
              <text
                key={`empty-${c.item.name}`}
                x={x + 10}
                y={HEADER_H + 30}
                className="fill-muted-foreground italic"
                style={{ fontFamily: "ui-monospace, monospace", fontSize: 10 }}
              >
                no CFG
              </text>,
            ];
          }
          const xOff = colX.get(c.item.name)!;
          return c.item.cfg.nodes.map((n) => {
            const p = c.layout.positions.get(n.id);
            if (!p) return null;
            const k = (n.kind as { type: string }).type;
            const label = CFG_KIND_LABEL[k] ?? k;
            const fill = CFG_KIND_FILL[k] ?? "fill-muted/30 stroke-border";
            const x = xOff + p.x;
            const y = HEADER_H + p.y;
            return (
              <g key={`cfgn-${c.item.name}-${n.id}`}>
                <title>{`${k}  span ${n.span.start}..${n.span.end}`}</title>
                <rect
                  x={x}
                  y={y}
                  width={CFG_NODE_W}
                  height={CFG_NODE_H}
                  rx={k === "entry" || k === "exit" ? 14 : 4}
                  className={fill + " stroke-[1.2]"}
                />
                <text
                  x={x + CFG_NODE_W / 2}
                  y={y + CFG_NODE_H / 2 + 3}
                  textAnchor="middle"
                  className="fill-foreground"
                  style={{ fontFamily: "ui-monospace, monospace", fontSize: 10 }}
                >
                  {label}
                </text>
              </g>
            );
          });
        })}

        {/* Inter-column call edges — column right edge → next column entry */}
        {callEdges.map((e, i) => {
          const fromX = colX.get(e.from);
          const toX = colX.get(e.to);
          if (fromX === undefined || toX === undefined) return null;
          const fromCol = cols.find((c) => c.item.name === e.from);
          const toCol = cols.find((c) => c.item.name === e.to);
          if (!fromCol || !toCol) return null;
          const x1 = fromX + fromCol.layout.width;
          const y1 = colEntryY.get(e.from) ?? HEADER_H + 12;
          const x2 = toX;
          const y2 = colEntryY.get(e.to) ?? HEADER_H + 12;
          // Curved cubic Bezier so multiple call edges read clearly.
          const dx = (x2 - x1) / 2;
          const path = `M ${x1} ${y1} C ${x1 + dx} ${y1}, ${x2 - dx} ${y2}, ${x2} ${y2}`;
          return (
            <path
              key={`call-${i}`}
              d={path}
              fill="none"
              className={
                e.side === "base"
                  ? "stroke-rose-400/60 stroke-[1.4]"
                  : "stroke-sky-500/70 stroke-[1.4]"
              }
              strokeDasharray={e.side === "base" ? "4 3" : undefined}
              markerEnd={e.side === "base" ? "url(#cfg-arrow)" : "url(#cfg-call-arrow)"}
            />
          );
        })}
      </svg>
    </div>
  );
}

function findNodeByName(
  nodes: import("@/types/artifact").Node[],
  qname: string,
): import("@/types/artifact").Node | null {
  for (const n of nodes) {
    const k = n.kind as { type: string; name?: string; path?: string };
    if (k.type === "function" || k.type === "type" || k.type === "state") {
      if (k.name === qname) return n;
    } else if (k.type === "api-endpoint") {
      if (k.path === qname) return n;
    } else if (k.type === "file") {
      if (k.path === qname) return n;
    }
  }
  return null;
}

function nodeSignature(n: import("@/types/artifact").Node): string {
  const k = n.kind as { type: string; signature?: string; name?: string; path?: string; method?: string };
  if (k.type === "function" && k.signature) return k.signature;
  if (k.type === "api-endpoint") return `${k.method ?? ""} ${k.path ?? ""}`.trim();
  if (k.type === "type" || k.type === "state") return k.name ?? "";
  if (k.type === "file") return k.path ?? "";
  return "";
}

/** Per-flow Delta view — one ordered card list of every signed
 *  observation that touches this flow. Three sources merge:
 *    - cost drivers (signed nav-cost units, sorted by |value|)
 *    - evidence claims (typed observations from the deterministic pass)
 *    - per-claim proof statuses (when intent + proof passes ran)
 *
 *  Reviewer reads top-down: biggest movers first, then context. No
 *  hidden categories — empty sources just don't render. Cost +/-
 *  is color-tinted, evidence by strength, proof by status. */
function FlowDelta({ flow }: { artifact: Artifact; flow: Flow }) {
  const cost = flow.cost ?? null;
  const drivers = (cost?.drivers ?? []).slice().sort((a, b) => Math.abs(b.value) - Math.abs(a.value));
  const evidence = (flow.evidence ?? []).slice().sort((a, b) => strengthRank(b.strength) - strengthRank(a.strength));
  const proofClaims = flow.proof?.claims ?? [];
  const empty = drivers.length === 0 && evidence.length === 0 && proofClaims.length === 0;

  return (
    <div className="space-y-4">
      <header className="space-y-1">
        <h1 className="text-[15px] font-mono text-foreground">
          Delta
          <span className="font-normal text-muted-foreground"> · scoped to {flow.name}</span>
        </h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Every signed observation that touches this flow, ordered by impact.
          Cost drivers first; evidence and proof claims follow.
        </p>
      </header>

      {empty && (
        <p className="text-[12px] text-muted-foreground">
          No observations on this flow yet.
        </p>
      )}

      {drivers.length > 0 && (
        <section className="space-y-2">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Cost drivers ({drivers.length})
          </h2>
          <ul className="space-y-1.5">
            {drivers.map((d, i) => (
              <li key={`drv-${i}`}>
                <DeltaCard
                  tone={d.value > 0 ? "bad" : d.value < 0 ? "good" : "neutral"}
                  badge={`${d.value > 0 ? "+" : d.value < 0 ? "\u2212" : ""}${Math.abs(d.value)}`}
                  label={d.label}
                  detail={d.detail}
                />
              </li>
            ))}
          </ul>
        </section>
      )}

      {evidence.length > 0 && (
        <section className="space-y-2">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Evidence ({evidence.length})
          </h2>
          <ul className="space-y-1.5">
            {evidence.map((c) => (
              <li key={c.id}>
                <DeltaCard
                  tone={
                    c.strength === "high"
                      ? "good"
                      : c.strength === "low"
                        ? "partial"
                        : "neutral"
                  }
                  badge={c.kind}
                  label={c.text}
                  detail={(c.entities ?? []).join(", ")}
                />
              </li>
            ))}
          </ul>
        </section>
      )}

      {proofClaims.length > 0 && (
        <section className="space-y-2">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Proof claims ({proofClaims.length})
          </h2>
          <ul className="space-y-1.5">
            {proofClaims.map((c, i) => (
              <li key={`pc-${i}`}>
                <DeltaCard
                  tone={
                    c.status === "found"
                      ? "good"
                      : c.status === "partial"
                        ? "partial"
                        : "bad"
                  }
                  badge={c.status}
                  label={c.statement || `claim #${c.claim_index}`}
                  detail={(c.evidence ?? []).map((e) => e.detail).join(" · ")}
                />
              </li>
            ))}
          </ul>
        </section>
      )}
    </div>
  );
}

const TONE_BORDER: Record<"good" | "partial" | "bad" | "neutral", string> = {
  good:
    "border-l-emerald-500/70 bg-emerald-50/60 dark:bg-emerald-400/[0.05]",
  partial:
    "border-l-amber-500/70 bg-amber-50/60 dark:bg-amber-400/[0.05]",
  bad:
    "border-l-rose-500/70 bg-rose-50/60 dark:bg-rose-400/[0.05]",
  neutral: "border-l-border/60 bg-muted/10",
};

function DeltaCard({
  tone,
  badge,
  label,
  detail,
}: {
  tone: "good" | "partial" | "bad" | "neutral";
  badge: string;
  label: string;
  detail?: string;
}) {
  return (
    <div
      className={
        "rounded border border-border/60 border-l-[3px] px-3 py-2 space-y-0.5 " +
        TONE_BORDER[tone]
      }
    >
      <div className="flex items-baseline gap-2">
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground tabular-nums">
          {badge}
        </span>
        <span className="text-[12px] text-foreground leading-snug">
          {label}
        </span>
      </div>
      {detail && (
        <p className="text-[10px] font-mono text-muted-foreground truncate">
          {detail}
        </p>
      )}
    </div>
  );
}

function strengthRank(s: string | undefined): number {
  if (s === "high") return 3;
  if (s === "medium") return 2;
  if (s === "low") return 1;
  return 0;
}


/**
 * Per-flow Intent & Proof view. Two sections side-by-side in the header:
 *   - Intent-fit: did this flow deliver something the PR intent claims?
 *   - Proof: is there real evidence (benchmarks, examples, claim-asserting tests)?
 *
 * Proof claims are rendered individually so the reviewer can see exactly
 * which intent claim is verified vs. missing — per the RFC and
 * `feedback_proof_not_tests.md`, this is what proof actually means.
 */
function FlowProof({ artifact, flow }: { artifact: Artifact; flow: Flow }) {
  const status = artifact.proof_status ?? "not-run";
  const hasIntent = artifact.intent != null;

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">
          Intent & Proof · analysing
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl">
          Running intent-fit + proof-verification on this flow. GLM sessions
          are scanning the repo for example files, claim-asserting tests, and
          reviewer-supplied benchmark notes. Keep working in other tabs — this
          fills in when ready.
        </p>
      </div>
    );
  }

  if (status === "not-run") {
    return (
      <div className="space-y-3">
        <h2 className="text-[13px] font-mono text-foreground">
          Intent & Proof
        </h2>
        {!hasIntent ? (
          <p className="text-[12px] text-muted-foreground max-w-3xl">
            No intent supplied for this PR. Pass{" "}
            <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">
              intent
            </code>{" "}
            (structured or raw text) or{" "}
            <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">
              --intent-file
            </code>{" "}
            /{" "}
            <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">
              --intent-pr
            </code>{" "}
            on the CLI — without an intent the passes have nothing to verify
            against.
          </p>
        ) : (
          <p className="text-[12px] text-muted-foreground max-w-3xl">
            Proof unavailable — configure{" "}
            <code className="mx-1 rounded bg-muted/50 px-1 text-[11px] font-mono">
              ADR_GLM_API_KEY
            </code>{" "}
            (GLM is the recommended backend for intent + proof) and re-run.
          </p>
        )}
      </div>
    );
  }

  const fit = flow.intent_fit ?? null;
  const proof = flow.proof ?? null;

  return (
    <section className="space-y-5">
      {artifact.intent && <IntentCard intent={artifact.intent} />}
      {status === "errored" && (
        <div className="rounded border border-border/60 bg-muted/20 px-3 py-2 text-[11px] font-mono text-muted-foreground">
          Proof pass reported errors on at least one flow — partial claims may
          be shown. Server logs hold the specific failures.
        </div>
      )}
      <div className="grid grid-cols-1 lg:grid-cols-2 gap-4">
        <IntentFitCard fit={fit} />
        <ProofCard proof={proof} />
      </div>
      {proof && proof.claims && proof.claims.length > 0 && (
        <section className="space-y-3">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
            Proof · per-claim breakdown ({proof.claims.length})
          </h2>
          <ol className="space-y-2">
            {proof.claims.map((c, i) => (
              <li key={i}>
                <ClaimProofRow claim={c} />
              </li>
            ))}
          </ol>
        </section>
      )}
    </section>
  );
}

function IntentCard({
  intent,
}: {
  intent: import("@/types/artifact").IntentInput;
}) {
  // IntentInput is Intent | string.
  const isStructured = typeof intent !== "string";
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2">
      <div className="flex items-baseline gap-2">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Intent
        </h2>
        <span className="text-[10px] font-mono text-muted-foreground">
          {isStructured ? "structured" : "raw text"}
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
            <ol className="mt-2 space-y-1.5">
              {intent.claims.map((c, i) => (
                <li
                  key={i}
                  className="flex items-baseline gap-2 text-[12px]"
                >
                  <span className="text-[10px] font-mono text-muted-foreground tabular-nums w-5">
                    {i}.
                  </span>
                  <span className="text-[10px] font-mono uppercase text-muted-foreground w-[4.5rem]">
                    {c.evidence_type}
                  </span>
                  <span className="text-foreground">{c.statement}</span>
                </li>
              ))}
            </ol>
          )}
        </div>
      ) : (
        <pre className="text-[12px] text-foreground whitespace-pre-wrap font-sans max-h-48 overflow-y-auto">
          {intent}
        </pre>
      )}
    </section>
  );
}

function IntentFitCard({
  fit,
}: {
  fit: import("@/types/artifact").IntentFit | null;
}) {
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2.5">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Intent fit
        </h2>
        {fit && (
          <span
            className="text-[10px] font-mono text-muted-foreground"
            title={`${fit.model}@${fit.prompt_version}`}
          >
            {fit.model}
          </span>
        )}
      </div>
      {!fit ? (
        <p className="text-[12px] text-muted-foreground">
          No verdict — the intent-fit pass didn't emit a parseable result for
          this flow.
        </p>
      ) : (
        <div className="space-y-2">
          <div className="flex items-baseline gap-2">
            <VerdictPill verdict={fit.verdict} kind="fit" />
            <StrengthPips strength={fit.strength} />
          </div>
          <p className="text-[12px] text-foreground leading-relaxed">
            {fit.reasoning}
          </p>
          {fit.matched_claims && fit.matched_claims.length > 0 && (
            <p className="text-[10px] font-mono text-muted-foreground">
              matched claims:{" "}
              {fit.matched_claims.map((n) => `#${n}`).join(" ")}
            </p>
          )}
        </div>
      )}
    </section>
  );
}

function ProofCard({
  proof,
}: {
  proof: import("@/types/artifact").Proof | null;
}) {
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2.5">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Proof
        </h2>
        {proof && (
          <span
            className="text-[10px] font-mono text-muted-foreground"
            title={`${proof.model}@${proof.prompt_version}`}
          >
            {proof.model}
          </span>
        )}
      </div>
      {!proof ? (
        <p className="text-[12px] text-muted-foreground">
          No verdict — the proof-verification pass didn't emit a parseable
          result for this flow.
        </p>
      ) : (
        <div className="space-y-2">
          <div className="flex items-baseline gap-2">
            <VerdictPill verdict={proof.verdict} kind="proof" />
            <StrengthPips strength={proof.strength} />
          </div>
          <p className="text-[12px] text-foreground leading-relaxed">
            {proof.reasoning}
          </p>
        </div>
      )}
    </section>
  );
}

function ClaimProofRow({
  claim,
}: {
  claim: import("@/types/artifact").ClaimProofStatus;
}) {
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2.5 space-y-1.5">
      <div className="flex items-baseline gap-2">
        <VerdictPill verdict={claim.status} kind="claim" small />
        <StrengthPips strength={claim.strength} />
        <span
          className="text-[10px] font-mono text-muted-foreground"
          title={`claim index: ${claim.claim_index}`}
        >
          #{claim.claim_index < 0 ? "raw" : claim.claim_index}
        </span>
      </div>
      <p className="text-[12px] text-foreground leading-relaxed">
        {claim.statement}
      </p>
      {claim.evidence && claim.evidence.length > 0 && (
        <ul className="mt-1 space-y-1">
          {claim.evidence.map((e, i) => (
            <li
              key={i}
              className="text-[11px] text-muted-foreground leading-relaxed"
            >
              <span className="inline-block w-[4.5rem] text-[10px] font-mono uppercase tracking-wide">
                {e.evidence_type}
              </span>
              <span>{e.detail}</span>
              {e.path && (
                <code className="ml-2 rounded bg-background/60 border border-border/50 px-1 text-[10px] font-mono text-foreground/80">
                  {e.path}
                </code>
              )}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function VerdictPill({
  verdict,
  kind,
  small,
}: {
  verdict: string;
  kind: "fit" | "proof" | "claim";
  small?: boolean;
}) {
  const size = small ? "text-[10px] px-1.5 py-0.5" : "text-[11px] px-2 py-0.5";
  const tone =
    kind === "fit"
      ? intentFitPillClass(verdict as IntentFitVerdict)
      : kind === "proof"
        ? proofPillClass(verdict as ProofVerdict)
        : claimPillClass(verdict);
  return (
    <span className={`${size} rounded-full border font-mono uppercase tracking-wide ${tone}`}>
      {verdict}
    </span>
  );
}

/** ClaimProofKind = "found" | "partial" | "missing". Maps to the same
 *  good / partial / bad vocabulary as fit/proof pills. */
function claimPillClass(status: string): string {
  if (status === "found") {
    return "border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200";
  }
  if (status === "partial") {
    return "border-amber-400/40 bg-amber-50 text-amber-800 dark:bg-amber-400/10 dark:text-amber-200";
  }
  if (status === "missing") {
    return "border-rose-400/40 bg-rose-50 text-rose-800 dark:bg-rose-400/10 dark:text-rose-200";
  }
  return "border-border/60 bg-background/60 text-muted-foreground";
}
