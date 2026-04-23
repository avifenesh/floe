import { useEffect, useMemo } from "react";
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
import { LoadingDots } from "@/components/LoadingDots";
import { FLOW_SUB_TABS } from "./types";
import { SourceView } from "./source";
import { InlineNotes } from "@/components/InlineNotes";

interface Props {
  artifact: Artifact;
  jobId: string;
  flow: Flow;
  sub: FlowSubTab;
  onInlineNotesChange?: (next: import("@/types/artifact").InlineNote[]) => void;
  /** Called when a reviewer clicks "→ source" on a driver row. Lets
   *  the parent flip the flow sub-tab to `source` and optionally
   *  scroll to a specific entity. */
  onJumpToSource?: (entity?: string) => void;
}

/**
 * Flow workspace. Each flow has its own set of sub-tabs; we render one at
 * a time based on the current sub selection. Every sub-tab is fully
 * implemented: Overview (header + hunks), Flow (graph visualization),
 * Morph (intent vs. result), Delta (cost drivers + evidence + proof
 * ordered by impact), Evidence (claim list), Source (flow-scoped diff),
 * Cost (axes + drivers + proof peer), Intent & Proof (verdict cards).
 */
export function FlowWorkspace({
  artifact,
  jobId,
  flow,
  sub,
  onInlineNotesChange,
  onJumpToSource,
}: Props) {
  const order = FLOW_SUB_TABS.findIndex((t) => t.key === sub);
  const body = (() => {
    switch (sub) {
      case "overview":
        return <FlowOverview artifact={artifact} flow={flow} />;
      case "flow":
        return (
          <FlowGraph
            artifact={artifact}
            flow={flow}
            jobId={jobId}
            onInlineNotesChange={onInlineNotesChange}
          />
        );
      case "morph":
        return <FlowMorph artifact={artifact} flow={flow} />;
      case "delta":
        return <FlowDelta artifact={artifact} flow={flow} />;
      case "source":
        return (
          <FlowSource
            artifact={artifact}
            jobId={jobId}
            flow={flow}
            onInlineNotesChange={onInlineNotesChange}
          />
        );
      case "cost":
        return <FlowCost artifact={artifact} flow={flow} onJumpToSource={onJumpToSource} />;
      case "proof":
        return (
          <FlowProof
            artifact={artifact}
            flow={flow}
            jobId={jobId}
            onInlineNotesChange={onInlineNotesChange}
            onJumpToSource={onJumpToSource}
          />
        );
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
  onInlineNotesChange,
}: {
  artifact: Artifact;
  jobId: string;
  flow: Flow;
  onInlineNotesChange?: (next: import("@/types/artifact").InlineNote[]) => void;
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
  return (
    <SourceView
      artifact={artifact}
      jobId={jobId}
      scope={scope}
      onInlineNotesChange={onInlineNotesChange}
    />
  );
}

// FlowEvidence was merged into FlowProof per user request — one
// page covers both "did the PR deliver its intent?" (Proof cards +
// per-claim breakdown) and "cheap structural context" (Evidence
// section below). ClaimRow stays for reuse inside FlowProof.

function ClaimRow({
  claim,
  onJumpToSource,
}: {
  claim: import("@/types/artifact").Claim;
  onJumpToSource?: (ref: import("@/types/artifact").SourceRef) => void;
}) {
  const kindLabel = kindToLabel(claim.kind);
  const refs = claim.source_refs ?? [];
  return (
    <div className="rounded border border-border/60 bg-muted/20 px-3 py-2.5 space-y-1.5">
      <div className="flex items-baseline gap-2">
        <StrengthPips strength={claim.strength} />
        <span className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
          {kindLabel}
        </span>
        {refs.length > 0 && onJumpToSource && (
          <button
            type="button"
            onClick={() => onJumpToSource(refs[0])}
            title={
              refs.length === 1
                ? `Jump to ${refs[0].file}:${refs[0].line}`
                : `${refs.length} source ranges — jump to first`
            }
            className="ml-auto text-[10px] font-mono text-muted-foreground hover:text-foreground underline underline-offset-2 decoration-dotted hover:decoration-solid"
          >
            → source
            {refs.length > 1 && (
              <span className="ml-1 opacity-70">·{refs.length}</span>
            )}
          </button>
        )}
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
    case "coverage-drop":
      return "coverage";
  }
}

function FlowCost({
  artifact,
  flow,
  onJumpToSource,
}: {
  artifact: Artifact;
  flow: Flow;
  onJumpToSource?: (entity?: string) => void;
}) {
  const status = artifact.cost_status ?? "not-run";
  const cost = flow.cost;

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground inline-flex items-baseline gap-2">
          Cost
          <span className="text-[11px] text-muted-foreground normal-case font-sans inline-flex items-baseline gap-1">
            <LoadingDots />
            <span>analysing</span>
          </span>
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Measuring navigation cost on base and head snapshots in
          parallel. The delta lands here when both sides complete.
          Keep working in other tabs; it fills in when ready.
        </p>
      </div>
    );
  }

  if (status === "not-run") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Cost unavailable — this deployment isn't configured with a
          navigation probe. An administrator can enable it, or re-run
          once one is configured.
        </p>
      </div>
    );
  }

  if (status === "errored") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground">Cost · errored</h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
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
              className="ml-auto text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border border-amber-400/40 bg-amber-50 dark:bg-amber-400/10 text-amber-800 dark:text-amber-200"
            >
              low confidence
            </span>
          )}
          {/* Probe-model stamp deliberately lives in FlowBaselineStamp
              below (diagnostic metadata) — the hero row stays copy-only
              per the no-model-names-in-product-UI rule. */}
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
              <CostDriverRow
                driver={d}
                baseline={baseline}
                onJumpToSource={onJumpToSource}
                flow={flow}
              />
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
      className="rounded-md border border-border/60 bg-muted/60 px-3 py-2 space-y-1 shadow-sm"
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
            className="rounded-md border border-border/60 bg-muted/60 px-3 py-2.5 space-y-1.5 shadow-sm"
            title={
              denom && denom > 0
                ? `${it.key}: ${formatSigned(it.value)} of ${denom} baseline cost (${pctLabel})`
                : `${it.key}: no baseline denominator yet (probe may not map to this axis)`
            }
          >
            <div className="flex items-baseline justify-between">
              <span className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
                {it.key}
              </span>
              <span className="text-[13px] font-mono font-semibold tabular-nums text-foreground">
                {formatSigned(it.value)}
                <span className="ml-2 font-normal text-muted-foreground text-[10px]">
                  {pctLabel}
                </span>
              </span>
            </div>
            <div className="h-1.5 rounded-full bg-muted overflow-hidden relative mt-0.5">
              <div
                aria-hidden
                className="absolute top-0 bottom-0 w-px bg-border/60"
                style={{ left: "50%" }}
              />
              <div
                className="absolute top-0 h-full bg-foreground/55 rounded-full"
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
  onJumpToSource,
  flow,
}: {
  driver: import("@/types/artifact").CostDriver;
  baseline: import("@/types/artifact").ArtifactBaseline | null;
  onJumpToSource?: (entity?: string) => void;
  flow?: Flow;
}) {
  // Best-effort: pick an entity from the flow's roster that the
  // driver's `detail` mentions, and hand it to the Source view so the
  // reviewer lands near the movement. Falls back to a plain
  // sub-tab flip when no entity match is found.
  const jumpTarget = (() => {
    if (!flow) return undefined;
    const haystack = (driver.detail ?? "").toLowerCase();
    return (flow.entities ?? []).find((e) => haystack.includes(e.toLowerCase()));
  })();
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
    <div className="rounded-md border border-border/60 bg-muted/60 px-3 py-2 space-y-1.5 shadow-sm">
      <div className="flex items-baseline gap-3">
        <span className="text-[12px] text-foreground">{driver.label}</span>
        {onJumpToSource && (
          <button
            type="button"
            onClick={() => onJumpToSource(jumpTarget)}
            className="text-[10px] font-mono rounded border border-border/60 bg-background px-1.5 py-0.5 text-muted-foreground hover:text-foreground hover:border-border transition-colors"
            title={
              jumpTarget
                ? `Jump to source for ${jumpTarget}`
                : "Jump to the flow's source view"
            }
          >
            → source
          </button>
        )}
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
 * Driver labels come from `floe-cost` — "API-surface navigation",
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
    <div className="space-y-4">
      <header className="space-y-1">
        <h1 className="text-[15px] font-mono text-foreground">
          Morph
          <span className="font-normal text-muted-foreground"> · {flow.name}</span>
        </h1>
      </header>

      {/* Top-line verdict pills — subtle when empty, structured always. */}
      <section className="flex items-baseline gap-2 flex-wrap">
        <VerdictPillCompact
          label="fit"
          value={fit?.verdict ?? null}
          className={intentFitPillClass(fit?.verdict)}
        />
        <VerdictPillCompact
          label="proof"
          value={flow.proof?.verdict ?? null}
          className={proofPillClass(flow.proof?.verdict)}
        />
      </section>

      {/* Intent vs Result — two columns, one row per claim. */}
      {panels.length > 0 ? (
        <section className="rounded border border-border/60 overflow-hidden shadow-sm">
          <div className="grid grid-cols-2 text-[10px] font-mono uppercase tracking-wide text-muted-foreground bg-muted/20 border-b border-border/60">
            <div className="px-3 py-1.5">Intent</div>
            <div className="px-3 py-1.5 border-l border-border/60">Result</div>
          </div>
          <ul className="divide-y divide-border/60">
            {panels.map((p) => (
              <li key={`claim-${p.idx}`} className="grid grid-cols-2">
                <IntentCell text={p.text} evidenceType={p.evidenceType} />
                <ResultCell
                  proof={p.proof}
                  flowEntities={flow.entities ?? []}
                  entityStatus={entityStatus}
                />
              </li>
            ))}
          </ul>
        </section>
      ) : !intent ? (
        <p className="text-[12px] text-muted-foreground italic">
          No intent supplied — pass a PR description to see Intent vs Result.
        </p>
      ) : (
        <p className="text-[12px] text-muted-foreground italic">
          Intent + proof passes still running…
        </p>
      )}

      {/* Replacement pairs — refactor detection, separate section. */}
      {replacements.length > 0 && (
        <section className="space-y-1.5">
          <h2 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground">
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

      {/* Pre-intent summary hides under a disclosure — keeps the top
          clean but lets the curious reviewer see the PR's prose. */}
      {intentSummary && (
        <details className="text-[11px] text-muted-foreground">
          <summary className="cursor-pointer select-none hover:text-foreground">
            PR intent summary
          </summary>
          <p className="pl-3 pt-1 italic leading-relaxed">"{intentSummary}"</p>
        </details>
      )}
    </div>
  );
}

/** One verdict pill with a stable label prefix. When `value` is null,
 *  we still render the pill (subtle border + grayed '—') so the row
 *  has structure even before the pass completes. */
function VerdictPillCompact({
  label,
  value,
  className,
}: {
  label: string;
  value: string | null;
  className: string;
}) {
  const empty = value === null;
  const body = empty ? "—" : value;
  return (
    <span
      className={
        "inline-flex items-baseline gap-1.5 px-2 py-0.5 rounded-full border text-[10px] font-mono " +
        (empty
          ? "border-border/50 bg-muted/20 text-muted-foreground/70"
          : className)
      }
    >
      <span className="uppercase tracking-wide opacity-70">{label}</span>
      <span>·</span>
      <span className={empty ? "italic" : "font-medium"}>{body}</span>
    </span>
  );
}

function IntentCell({
  text,
  evidenceType,
}: {
  text: string;
  evidenceType: string;
}) {
  return (
    <div className="px-3 py-2.5 space-y-1">
      <p className="text-[12px] text-foreground leading-snug">{text}</p>
      <p className="text-[10px] font-mono text-muted-foreground uppercase tracking-wide">
        {evidenceType}
      </p>
    </div>
  );
}

function ResultCell({
  proof,
  flowEntities,
  entityStatus,
}: {
  proof: import("@/types/artifact").ClaimProofStatus | null;
  flowEntities: string[];
  entityStatus: Map<string, MorphStatus>;
}) {
  const status = proof?.status ?? null;
  const evidence = proof?.evidence ?? [];
  const deliverers = flowEntities.filter((name) =>
    evidence.some((e) => (e.detail ?? "").includes(name) || (e.path ?? "").includes(name)),
  );
  const tone =
    status === "found" ? "border-emerald-500/50" :
    status === "partial" ? "border-amber-500/50" :
    status === "missing" ? "border-rose-500/50" :
    "border-border/60";
  const label =
    status === "found" ? "delivered" :
    status === "partial" ? "partial" :
    status === "missing" ? "missing" :
    "—";
  const labelColor =
    status === "found" ? "text-emerald-700 dark:text-emerald-300" :
    status === "partial" ? "text-amber-700 dark:text-amber-300" :
    status === "missing" ? "text-rose-700 dark:text-rose-300" :
    "text-muted-foreground/70 italic";
  return (
    <div className={"px-3 py-2.5 border-l-[3px] " + tone + " space-y-1.5"}>
      <div className="flex items-baseline gap-2">
        <span className={"text-[11px] font-mono font-medium " + labelColor}>
          {label}
        </span>
      </div>
      {deliverers.length > 0 && (
        <div className="flex items-baseline gap-1 flex-wrap">
          {deliverers.map((name) => {
            const s = entityStatus.get(name) ?? "unchanged";
            return (
              <span
                key={name}
                className="inline-flex items-center gap-1 text-[10px] font-mono text-foreground rounded px-1 py-[1px] bg-background/60 border border-border/60"
                title={`${name} · ${s}`}
              >
                <span className={"w-1 h-1 rounded-full " + STATUS_DOT[s]} aria-hidden />
                {name}
              </span>
            );
          })}
        </div>
      )}
      {evidence.length > 0 && (
        <ul className="text-[10px] text-muted-foreground leading-snug space-y-0.5">
          {evidence.slice(0, 2).map((e, i) => (
            <li key={i} className="truncate" title={e.detail}>
              · {e.detail}
            </li>
          ))}
        </ul>
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

/** Per-flow Flow view (RFC view 02) — entity-level call tree.
 *  Each entity the flow touches becomes a card showing what
 *  changed at that entity (signature diff, new/removed variants,
 *  hunks); arrows run from callers to callees so the reviewer
 *  follows the runtime trajectory without having to decode a
 *  per-function CFG. Dropped the function-internal CFG pass
 *  because reviewers can't parse "entry / seq / branch / return"
 *  at a glance — per-entity summaries read immediately. CFG data
 *  still lives in the artifact; NodeDetailPanel surfaces it on
 *  click for a reviewer who wants that depth. */
function FlowGraph({
  artifact,
  flow,
  jobId,
  onInlineNotesChange,
}: {
  artifact: Artifact;
  flow: Flow;
  jobId: string;
  onInlineNotesChange?: (next: import("@/types/artifact").InlineNote[]) => void;
}) {
  const [selected, setSelected] = useState<string | null>(null);
  // Reviewer-managed reorder of entity cards. Starts at the flow's
  // natural entity order; drag-and-drop shuffles in-place. Resets
  // when the flow id changes so opening a different flow doesn't
  // inherit a stale layout.
  // Combined entity set: hunk-touched + LLM-supplied extras + any
  // propagation-edge endpoint. The flow graph should show the
  // whole component that carries this story end-to-end, not just
  // the delta — otherwise the reviewer can't see the surrounding
  // context that makes the change readable.
  //
  // Filter out File-kind entries: the analyzer sometimes emits a
  // File node as an "entity" (the module-scope container), but File
  // nodes aren't reviewer-facing call-graph participants — they'd
  // render as a standalone file path in the SVG which looks broken.
  const isRealEntity = (name: string): boolean => {
    const n = findNodeByName(artifact.head.nodes, name) ?? findNodeByName(artifact.base.nodes, name);
    if (!n) return true; // unresolved names (from propagation_edges) stay — they represent real entities we just don't have graph nodes for.
    const kind = (n.kind as { type?: string }).type;
    return kind !== "file";
  };
  const fullEntities: string[] = [];
  const pushEntity = (n: string) => {
    if (n && !fullEntities.includes(n) && isRealEntity(n)) fullEntities.push(n);
  };
  (flow.entities ?? []).forEach(pushEntity);
  (flow.extra_entities ?? []).forEach(pushEntity);
  for (const [from, to] of flow.propagation_edges ?? []) {
    pushEntity(from);
    pushEntity(to);
  }
  const [order, setOrder] = useState<string[]>(fullEntities);
  useEffect(() => {
    setOrder(fullEntities);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [flow.id]);
  const [dragName, setDragName] = useState<string | null>(null);
  const entities: string[] = order.filter((n) => fullEntities.includes(n));
  for (const n of fullEntities) {
    if (!entities.includes(n)) entities.push(n);
  }
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
    const file = (head ?? base)?.file ?? "";
    // Hunks that touch this entity — filtered out of the flow's
    // hunk list by entity reference through base/head node ids.
    const hunks = (artifact.hunks ?? []).filter((h) => {
      if (!flow.hunk_ids.includes(h.id)) return false;
      const k = h.kind as { kind: string; node?: number };
      if (k.kind === "state" || k.kind === "api") {
        return (
          k.node !== undefined &&
          ((head && head.id === k.node) || (base && base.id === k.node))
        );
      }
      if (k.kind === "call") {
        // Call hunks reference edges, not a single node. Include any
        // call hunk that has an edge with endpoint == this entity.
        const kc = k as unknown as {
          kind: "call";
          added_edges?: number[];
          removed_edges?: number[];
        };
        const allEdgeIds = [
          ...(kc.added_edges ?? []),
          ...(kc.removed_edges ?? []),
        ];
        const allEdges = [...(artifact.head.edges ?? []), ...(artifact.base.edges ?? [])];
        return allEdgeIds.some((eid) => {
          const e = allEdges.find((x) => x.id === eid);
          if (!e) return false;
          return (head && (e.from === head.id || e.to === head.id)) ||
                 (base && (e.from === base.id || e.to === base.id));
        });
      }
      return false;
    });
    return { name, base, head, status, file, hunks };
  });
  // Call edges between flow entities on each side. We collect the
  // full graph on base and head independently so the side-by-side
  // panels can each render their own snapshot — not just the
  // hunk-touched delta. `verb` is determined by which panel the
  // edge belongs to (base = may-be-removed, head = may-be-added).
  const entitySet = new Set(entities);
  const nameFor = (graph: Artifact["base"], id: number): string | null => {
    const n = graph.nodes.find((x) => x.id === id);
    if (!n) return null;
    const kind = n.kind as { type?: string; name?: string };
    return (kind.name as string | undefined) ?? null;
  };
  const collectPairs = (graph: Artifact["base"]): Array<{ from: string; to: string }> => {
    const out: Array<{ from: string; to: string }> = [];
    const seen = new Set<string>();
    for (const e of graph.edges ?? []) {
      if (e.kind !== "calls") continue;
      const f = nameFor(graph, e.from);
      const t = nameFor(graph, e.to);
      if (!f || !t || f === t) continue;
      if (!entitySet.has(f) || !entitySet.has(t)) continue;
      const k = `${f}|${t}`;
      if (seen.has(k)) continue;
      seen.add(k);
      out.push({ from: f, to: t });
    }
    return out;
  };
  const basePairs = collectPairs(artifact.base);
  const headPairs = collectPairs(artifact.head);
  // For legacy EntityGraph (2nd-tier consumers of callPairs below),
  // keep the added/removed diff list so nothing else breaks.
  const baseKeySet = new Set(basePairs.map((p) => `${p.from}|${p.to}`));
  const headKeySet = new Set(headPairs.map((p) => `${p.from}|${p.to}`));
  const callPairs: { from: string; to: string; verb: "added" | "removed" }[] = [];
  for (const p of headPairs) {
    if (!baseKeySet.has(`${p.from}|${p.to}`)) callPairs.push({ ...p, verb: "added" });
  }
  for (const p of basePairs) {
    if (!headKeySet.has(`${p.from}|${p.to}`)) callPairs.push({ ...p, verb: "removed" });
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
          <span className="font-normal text-muted-foreground"> · {flow.name}</span>
        </h1>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Base on the left, head on the right — each panel renders the
          full call graph for this flow in that snapshot. Entities the
          story touches <em>and</em> their unchanged neighbours appear,
          so you see the whole component end-to-end. Border color marks
          the entity's morph status between snapshots; added edges
          tint emerald on head, removed edges tint rose on base.
          Click any node to open its source.
        </p>
        <div className="flex items-baseline gap-3 text-[10px] font-mono text-muted-foreground pt-0.5">
          <MorphLegend status="added" count={counts.added} />
          <MorphLegend status="changed" count={counts.changed} />
          <MorphLegend status="removed" count={counts.removed} />
          <MorphLegend status="unchanged" count={counts.unchanged} />
        </div>
      </header>

      {items.length === 0 ? (
        <p className="text-[12px] text-muted-foreground italic">
          No entities resolved on this flow.
        </p>
      ) : (
        <>
          <PropagationStrip flow={flow} />
          <EntityGraph
            items={items}
            basePairs={basePairs}
            headPairs={headPairs}
            onSelect={(n) => setSelected(n)}
          />
          {/* Cards below the graph keep the per-entity hunk detail
              (signature diffs / variant lists / edge counts) the
              graph nodes can't fit. Drag-to-reorder stays here. */}
          <h2 className="text-[10px] font-mono uppercase tracking-wide text-muted-foreground pt-1">
            Per-entity detail
          </h2>
          <ul className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {items.map((it) => (
              <li
                key={it.name}
                draggable
                onDragStart={(e) => {
                  setDragName(it.name);
                  e.dataTransfer.effectAllowed = "move";
                  e.dataTransfer.setData("text/plain", it.name);
                }}
                onDragEnd={() => setDragName(null)}
                onDragOver={(e) => {
                  if (dragName && dragName !== it.name) e.preventDefault();
                }}
                onDrop={(e) => {
                  e.preventDefault();
                  if (!dragName || dragName === it.name) return;
                  setOrder((prev) => {
                    const names = [...prev];
                    const from = names.indexOf(dragName);
                    const to = names.indexOf(it.name);
                    if (from < 0 || to < 0) return prev;
                    names.splice(from, 1);
                    names.splice(to, 0, dragName);
                    return names;
                  });
                  setDragName(null);
                }}
                className={
                  "transition-opacity cursor-grab active:cursor-grabbing " +
                  (dragName === it.name ? "opacity-50" : "opacity-100")
                }
                title="Drag to reorder · click to open source"
              >
                <EntityCard
                  item={it}
                  onClick={() => setSelected(it.name)}
                />
                {onInlineNotesChange && (
                  <div className="mt-1 px-1">
                    <InlineNotes
                      jobId={jobId}
                      anchor={{ kind: "entity", entity_name: it.name }}
                      notes={artifact.inline_notes ?? []}
                      onChange={onInlineNotesChange}
                      label="note"
                    />
                  </div>
                )}
              </li>
            ))}
          </ul>
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

/** Entity-level call graph — SVG. Nodes are flow entities laid out
 *  in columns by topological level (caller → callee, left to right).
 *  Edges are colored by morph verb (added = emerald, removed = rose,
 *  unchanged = muted). Node border color marks the entity's own
 *  status (added / removed / changed / unchanged). Click a node to
 *  open the source panel for that entity.
 *
 *  Not draggable — auto-layout. The per-entity detail cards below
 *  are still drag-to-reorder for the reviewer's own reading order. */
function EntityGraph({
  items,
  basePairs,
  headPairs,
  onSelect,
}: {
  items: {
    name: string;
    status: MorphStatus;
    file: string;
    hunks: import("@/types/artifact").Hunk[];
    base?: import("@/types/artifact").Node | null;
    head?: import("@/types/artifact").Node | null;
  }[];
  basePairs: { from: string; to: string }[];
  headPairs: { from: string; to: string }[];
  onSelect: (entity: string) => void;
}) {
  // Layout: entities are listed top-to-bottom (one row per entity),
  // BASE column on the left, HEAD column on the right. Eye scans
  // down the list of entities and for each one sees base vs head at
  // a glance. Call edges within each column are drawn vertically —
  // a caller's row sits above its callee's (topological sort).
  //
  // Union of base + head edges drives the topological row order so
  // unchanged scaffolding sits in the right place regardless of
  // which snapshot carries the edge.
  const allPairs: { from: string; to: string }[] = [];
  const seenPair = new Set<string>();
  for (const p of [...basePairs, ...headPairs]) {
    const k = `${p.from}|${p.to}`;
    if (seenPair.has(k)) continue;
    seenPair.add(k);
    allPairs.push(p);
  }
  const incomingByName = new Map<string, string[]>();
  for (const it of items) incomingByName.set(it.name, []);
  for (const p of allPairs) {
    const inc = incomingByName.get(p.to);
    if (inc && !inc.includes(p.from) && p.from !== p.to) inc.push(p.from);
  }
  const levelCache = new Map<string, number>();
  const levelOf = (name: string, seen: Set<string> = new Set()): number => {
    if (levelCache.has(name)) return levelCache.get(name)!;
    if (seen.has(name)) return 0;
    seen.add(name);
    const inc = incomingByName.get(name) ?? [];
    const lv = inc.length === 0 ? 0 : 1 + Math.max(...inc.map((p) => levelOf(p, seen)));
    levelCache.set(name, lv);
    return lv;
  };
  // Stable row order: primary key = level (callers above callees);
  // tie-break = entity name for determinism.
  const sortedItems = [...items].sort((a, b) => {
    const la = levelOf(a.name);
    const lb = levelOf(b.name);
    if (la !== lb) return la - lb;
    return a.name.localeCompare(b.name);
  });
  const rowOf = new Map<string, number>();
  sortedItems.forEach((it, i) => rowOf.set(it.name, i));

  // Graph canvas tuned for desktop viewports. NODE_W is deliberately
  // generous so each box breathes; COL_GAP runs wide so the base/head
  // columns sit distinct rather than crowding at centre. Combined
  // natural width ~840px — scales cleanly up/down via SVG viewBox.
  const NODE_W = 320;
  const NODE_H = 60;
  const ROW_GAP = 16;
  const COL_GAP = 120;
  const PAD = 24;
  const HEADER_H = 26;
  const totalW = PAD * 2 + NODE_W * 2 + COL_GAP;
  const totalH = PAD + HEADER_H + sortedItems.length * (NODE_H + ROW_GAP) + PAD;

  const baseX = PAD;
  const headX = PAD + NODE_W + COL_GAP;
  const rowY = (row: number) => PAD + HEADER_H + row * (NODE_H + ROW_GAP);

  const renderNode = (
    it: (typeof items)[number],
    side: "base" | "head",
  ) => {
    const x = side === "base" ? baseX : headX;
    const y = rowY(rowOf.get(it.name)!);
    const present = side === "base" ? !!it.base : !!it.head;
    const nodeStatus: MorphStatus = !present ? "unchanged" : it.status;
    // 0.55 passes WCAG AA against both the light (#FCFBF8) and the
    // dark (0 0% 9%) backgrounds for the mono text + muted-foreground
    // pairing we use here. 0.35 was failing contrast on faded nodes.
    const opacity = present ? 1 : 0.55;
    const short = it.name.length > 30 ? it.name.slice(0, 29) + "\u2026" : it.name;
    const file = (it.file.split("/").pop() ?? it.file) || "";
    const badge =
      !present && it.status === "added" && side === "base"
        ? "+"
        : !present && it.status === "removed" && side === "head"
          ? "−"
          : it.status === "changed" && present
            ? "~"
            : null;
    return (
      <g
        key={`${side}-${it.name}`}
        onClick={() => onSelect(it.name)}
        className="cursor-pointer"
        style={{ opacity }}
      >
        <title>{`${it.name}\n${it.file}\nmorph: ${it.status}\npresent on ${side}: ${present}`}</title>
        <rect
          x={x}
          y={y}
          width={NODE_W}
          height={NODE_H}
          rx={6}
          className={"fill-background stroke-[1.3] " + ENTITY_GRAPH_BORDER[nodeStatus]}
          strokeDasharray={!present ? "4 3" : undefined}
        />
        <circle cx={x + 10} cy={y + 12} r={3} className={ENTITY_GRAPH_DOT[nodeStatus]} />
        <text
          x={x + 20}
          y={y + 16}
          className="fill-foreground"
          style={{ fontFamily: "ui-monospace, monospace", fontSize: 12, fontWeight: 500 }}
        >
          {short}
        </text>
        <text
          x={x + 12}
          y={y + 32}
          className="fill-muted-foreground"
          style={{ fontFamily: "ui-monospace, monospace", fontSize: 10 }}
        >
          {present ? it.status : side === "base" ? "not in base" : "not in head"}
          {file ? " · " + file : ""}
        </text>
        <text
          x={x + 12}
          y={y + 47}
          className="fill-muted-foreground"
          style={{ fontFamily: "ui-monospace, monospace", fontSize: 10 }}
        >
          {present ? `${it.hunks.length} hunk${it.hunks.length === 1 ? "" : "s"}` : "—"}
        </text>
        {badge && (
          <text
            x={x + NODE_W - 14}
            y={y + 18}
            textAnchor="middle"
            className={
              badge === "+"
                ? "fill-emerald-500"
                : badge === "−"
                  ? "fill-rose-500"
                  : "fill-amber-500"
            }
            style={{ fontFamily: "ui-monospace, monospace", fontSize: 14, fontWeight: 600 }}
          >
            {badge}
          </text>
        )}
      </g>
    );
  };

  // Vertical call edges within a column. Use curved bezier from
  // bottom-centre of the caller node to top-centre of the callee.
  const renderEdges = (
    side: "base" | "head",
    pairs: { from: string; to: string }[],
  ) => {
    const x = side === "base" ? baseX : headX;
    const cx = x + NODE_W / 2;
    const otherKeys = new Set(
      (side === "base" ? headPairs : basePairs).map((p) => `${p.from}|${p.to}`),
    );
    return pairs
      .filter((p) => rowOf.has(p.from) && rowOf.has(p.to))
      .map((p, i) => {
        const rFrom = rowOf.get(p.from)!;
        const rTo = rowOf.get(p.to)!;
        const y1 = rowY(rFrom) + NODE_H;
        const y2 = rowY(rTo);
        const midY = (y1 + y2) / 2;
        const delta = !otherKeys.has(`${p.from}|${p.to}`);
        const stroke = delta
          ? side === "head"
            ? "stroke-emerald-500/75"
            : "stroke-rose-500/75"
          : "stroke-muted-foreground/40";
        const arrow = `url(#eg-arrow-${side}-${delta ? "delta" : "neutral"})`;
        return (
          <path
            key={`${side}-e-${i}`}
            d={`M ${cx},${y1} C ${cx},${midY} ${cx},${midY} ${cx},${y2}`}
            fill="none"
            strokeWidth={delta ? 1.4 : 1}
            className={stroke}
            strokeDasharray={delta && side === "base" ? "5 3" : undefined}
            markerEnd={arrow}
          />
        );
      });
  };

  return (
    <div className="relative w-full min-w-0 max-w-full overflow-y-auto rounded border border-border/60 bg-muted/10 shadow-sm">
      {/* Mobile fallback: below md the SVG scales down so far that
          entity names become unreadable. Render a stacked list
          (one row per entity, base/head status chips inline) —
          same data, fit for narrow viewports. */}
      <ul className="md:hidden divide-y divide-border/40">
        {sortedItems.map((it) => (
          <li
            key={it.name}
            className="flex items-baseline gap-2 px-3 py-2 cursor-pointer hover:bg-muted/30"
            onClick={() => onSelect(it.name)}
          >
            <span className="text-[12px] font-mono font-medium text-foreground truncate flex-1 min-w-0">
              {it.name}
            </span>
            <SideStatusChip present={!!it.base} label="base" status={it.status} />
            <SideStatusChip present={!!it.head} label="head" status={it.status} />
          </li>
        ))}
      </ul>
      <svg
        viewBox={`0 0 ${totalW} ${totalH}`}
        className="hidden md:block w-full"
        preserveAspectRatio="xMidYMin meet"
        style={{ maxHeight: `${totalH * 1.5}px` }}
      >
        <defs>
          <marker
            id="eg-arrow-base-delta"
            viewBox="0 0 10 10"
            refX="5"
            refY="9"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 0 L 5 10 z" className="fill-rose-500/80" />
          </marker>
          <marker
            id="eg-arrow-base-neutral"
            viewBox="0 0 10 10"
            refX="5"
            refY="9"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 0 L 5 10 z" className="fill-muted-foreground/50" />
          </marker>
          <marker
            id="eg-arrow-head-delta"
            viewBox="0 0 10 10"
            refX="5"
            refY="9"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 0 L 5 10 z" className="fill-emerald-500/80" />
          </marker>
          <marker
            id="eg-arrow-head-neutral"
            viewBox="0 0 10 10"
            refX="5"
            refY="9"
            markerWidth="7"
            markerHeight="7"
            orient="auto"
          >
            <path d="M 0 0 L 10 0 L 5 10 z" className="fill-muted-foreground/50" />
          </marker>
        </defs>
        {/* column headers */}
        <text
          x={baseX + NODE_W / 2}
          y={PAD + 14}
          textAnchor="middle"
          className="fill-muted-foreground"
          style={{ fontFamily: "ui-monospace, monospace", fontSize: 10, letterSpacing: 1 }}
        >
          BASE
        </text>
        <text
          x={headX + NODE_W / 2}
          y={PAD + 14}
          textAnchor="middle"
          className="fill-muted-foreground"
          style={{ fontFamily: "ui-monospace, monospace", fontSize: 10, letterSpacing: 1 }}
        >
          HEAD
        </text>
        {/* edges behind nodes */}
        {renderEdges("base", basePairs)}
        {renderEdges("head", headPairs)}
        {/* node rows */}
        {sortedItems.map((it) => (
          <g key={it.name}>
            {renderNode(it, "base")}
            {renderNode(it, "head")}
          </g>
        ))}
      </svg>
    </div>
  );
}

const ENTITY_GRAPH_BORDER: Record<MorphStatus, string> = {
  added: "stroke-emerald-500/70",
  removed: "stroke-rose-500/70",
  changed: "stroke-amber-500/70",
  unchanged: "stroke-border",
};
const ENTITY_GRAPH_DOT: Record<MorphStatus, string> = {
  added: "fill-emerald-500",
  removed: "fill-rose-500",
  changed: "fill-amber-500",
  unchanged: "fill-muted-foreground/50",
};

/** One entity card for the entity-centric Flow view. Shows name +
 *  file + morph-status pill at top, and a compact list of "what
 *  changed here" below (signature deltas, new/removed variants,
 *  inbound call edges). Clickable — opens NodeDetailPanel. */
function EntityCard({
  item,
  onClick,
}: {
  item: {
    name: string;
    base: import("@/types/artifact").Node | undefined | null;
    head: import("@/types/artifact").Node | undefined | null;
    status: MorphStatus;
    file: string;
    hunks: import("@/types/artifact").Hunk[];
  };
  onClick: () => void;
}) {
  const file = item.file.split("/").pop() ?? item.file;
  return (
    <button
      type="button"
      onClick={onClick}
      className={
        "w-full text-left rounded-md border-l-[3px] " +
        MORPH_BORDER_LEFT[item.status] +
        " border border-border/60 bg-muted/10 hover:bg-muted/20 px-3 py-2.5 space-y-1.5 transition-colors"
      }
    >
      <div className="flex items-baseline gap-2">
        <span className="text-[12px] font-mono font-medium text-foreground truncate">
          {item.name}
        </span>
        <span className={"text-[10px] font-mono uppercase tracking-wide " + MORPH_TEXT[item.status]}>
          {item.status}
        </span>
        <span className="ml-auto text-[10px] font-mono text-muted-foreground truncate min-w-0">
          {file}
        </span>
      </div>
      {item.hunks.length > 0 ? (
        <ul className="space-y-0.5">
          {item.hunks.map((h) => (
            <li key={h.id} className="text-[11px] font-mono text-foreground leading-snug">
              <HunkLine hunk={h} entity={item.name} />
            </li>
          ))}
        </ul>
      ) : (
        <p className="text-[11px] text-muted-foreground italic">
          Reached via propagation — no direct hunks.
        </p>
      )}
    </button>
  );
}

/** Compact side-status chip for the mobile flow-graph fallback.
 *  Shows whether an entity is present on this snapshot and, when
 *  present, hints the morph status (added/removed/changed) via the
 *  same tone tokens the SVG nodes use. Not rendered on ≥md. */
function SideStatusChip({
  present,
  label,
  status,
}: {
  present: boolean;
  label: "base" | "head";
  status: MorphStatus;
}) {
  const relevant =
    (label === "base" && status === "removed") ||
    (label === "head" && status === "added") ||
    status === "changed";
  const tone = !present
    ? "border-border/40 bg-background/60 text-muted-foreground/70"
    : relevant
      ? status === "added"
        ? "border-emerald-400/50 bg-emerald-50 text-emerald-700 dark:bg-emerald-400/10 dark:text-emerald-300"
        : status === "removed"
          ? "border-rose-400/50 bg-rose-50 text-rose-700 dark:bg-rose-400/10 dark:text-rose-300"
          : "border-amber-400/50 bg-amber-50 text-amber-700 dark:bg-amber-400/10 dark:text-amber-300"
      : "border-border/60 bg-background/60 text-muted-foreground";
  return (
    <span
      className={
        "text-[9px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded border " +
        tone
      }
    >
      {label}
      {present ? "" : " ·"}
    </span>
  );
}

const MORPH_BORDER_LEFT: Record<MorphStatus, string> = {
  added: "border-l-emerald-500/60",
  removed: "border-l-rose-500/60",
  changed: "border-l-amber-500/60",
  unchanged: "border-l-border/60",
};
const MORPH_TEXT: Record<MorphStatus, string> = {
  added: "text-emerald-700 dark:text-emerald-300",
  removed: "text-rose-700 dark:text-rose-300",
  changed: "text-amber-700 dark:text-amber-300",
  unchanged: "text-muted-foreground",
};

/** Per-hunk chip row inside an EntityCard. Prose ("call: +2 -1 edges
 *  touching X") was hard to scan alongside the morph legend; chips
 *  match the visual vocabulary of the flow graph and let the eye
 *  pattern-match what changed. */
function HunkLine({
  hunk,
}: {
  hunk: import("@/types/artifact").Hunk;
  entity: string;
}) {
  const k = hunk.kind as {
    kind: string;
    before_signature?: string | null;
    after_signature?: string | null;
    added_variants?: string[];
    removed_variants?: string[];
    added_edges?: number[];
    removed_edges?: number[];
  };
  if (k.kind === "api") {
    if (k.before_signature && k.after_signature) {
      return (
        <div className="flex flex-wrap gap-1">
          <Chip tone="rem">{"−" + k.before_signature}</Chip>
          <Chip tone="add">{"+" + k.after_signature}</Chip>
        </div>
      );
    }
    return (
      <div className="flex flex-wrap gap-1">
        <Chip tone="neutral">api · signature change</Chip>
      </div>
    );
  }
  if (k.kind === "state") {
    const adds = k.added_variants ?? [];
    const rems = k.removed_variants ?? [];
    return (
      <div className="flex flex-wrap gap-1">
        {adds.map((v) => (
          <Chip key={`a-${v}`} tone="add">{"+" + v}</Chip>
        ))}
        {rems.map((v) => (
          <Chip key={`r-${v}`} tone="rem">{"−" + v}</Chip>
        ))}
      </div>
    );
  }
  if (k.kind === "call") {
    const added = (k.added_edges ?? []).length;
    const removed = (k.removed_edges ?? []).length;
    return (
      <div className="flex flex-wrap gap-1">
        {added > 0 && (
          <Chip tone="add">{`+${added} call${added === 1 ? "" : "s"}`}</Chip>
        )}
        {removed > 0 && (
          <Chip tone="rem">{`−${removed} call${removed === 1 ? "" : "s"}`}</Chip>
        )}
      </div>
    );
  }
  return (
    <div className="flex flex-wrap gap-1">
      <Chip tone="neutral">hunk</Chip>
    </div>
  );
}

function Chip({
  tone,
  children,
}: {
  tone: "add" | "rem" | "neutral";
  children: import("react").ReactNode;
}) {
  const cls =
    tone === "add"
      ? "border-emerald-400/40 bg-emerald-50 text-emerald-800 dark:bg-emerald-400/10 dark:text-emerald-200"
      : tone === "rem"
        ? "border-rose-400/40 bg-rose-50 text-rose-800 dark:bg-rose-400/10 dark:text-rose-200"
        : "border-border/60 bg-background/60 text-muted-foreground";
  return (
    <span
      className={
        "inline-flex items-center text-[10px] font-mono px-1.5 py-0.5 rounded-full border " +
        cls
      }
    >
      {children}
    </span>
  );
}

// FlowCallSummary removed — EntityGraph renders the call pairs as
// SVG arrows now, which is the same information in a more honest
// shape.

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

type MorphStatus = "added" | "removed" | "changed" | "unchanged";

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
function FlowProof({
  artifact,
  flow,
  jobId,
  onInlineNotesChange,
  onJumpToSource,
}: {
  artifact: Artifact;
  flow: Flow;
  jobId?: string;
  onInlineNotesChange?: (next: import("@/types/artifact").InlineNote[]) => void;
  onJumpToSource?: (entity?: string) => void;
}) {
  const status = artifact.proof_status ?? "not-run";
  const hasIntent = artifact.intent != null;

  if (status === "analyzing") {
    return (
      <div className="space-y-2">
        <h2 className="text-[13px] font-mono text-foreground inline-flex items-baseline gap-2">
          Intent &amp; Proof
          <span className="text-[11px] text-muted-foreground normal-case font-sans inline-flex items-baseline gap-1">
            <LoadingDots />
            <span>analysing</span>
          </span>
        </h2>
        <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
          Matching this flow to the PR's stated intent, then hunting for
          evidence — example files, claim-asserting tests, reviewer notes.
          Keep working in other tabs; results fill in when ready.
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
          <p className="text-[12px] text-muted-foreground max-w-3xl leading-relaxed">
            Proof unavailable — this deployment isn't configured with an
            LLM backend for intent-fit or proof verification. An
            administrator can enable it, or re-run after configuring one.
          </p>
        )}
      </div>
    );
  }

  const fit = flow.intent_fit ?? null;
  const proof = flow.proof ?? null;

  return (
    <section className="space-y-5">
      {artifact.intent && (
        <IntentCard
          intent={artifact.intent}
          jobId={jobId}
          notes={artifact.inline_notes ?? []}
          onInlineNotesChange={onInlineNotesChange}
        />
      )}
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
      {jobId && onInlineNotesChange && (
        <section className="space-y-1">
          <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
            Reviewer notes on this flow
          </h2>
          <InlineNotes
            jobId={jobId}
            anchor={{ kind: "flow", flow_id: flow.id }}
            notes={artifact.inline_notes ?? []}
            onChange={onInlineNotesChange}
            label="note on this flow"
          />
        </section>
      )}
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
      {/* Structural evidence — file scope, call-chain, signature
          consistency, test coverage. Lives alongside proof because
          both answer "what's the context + evidence for this flow?"
          Cheap observations from the analyzer, not the LLM. */}
      {(flow.evidence ?? []).length > 0 && (
        <section className="space-y-2">
          <div>
            <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide">
              Structural evidence ({(flow.evidence ?? []).length})
            </h2>
            <p className="text-[11px] text-muted-foreground max-w-3xl leading-relaxed">
              Cheap context from the analyzer — file scope, call-chain
              connectedness, signature consistency, test coverage. Not
              a verdict on intent; that's above.
            </p>
          </div>
          <ol className="space-y-2">
            {(flow.evidence ?? []).map((c) => (
              <li key={c.id}>
                <ClaimRow
                  claim={c}
                  onJumpToSource={
                    onJumpToSource
                      ? (ref) => {
                          // Jump carries the entity name so the Source
                          // tab can scroll to the same name via the
                          // `data-entity-name` attribute wired in
                          // App.tsx. SourceRef's `file` is secondary;
                          // using the first entity keeps it consistent
                          // with the cost-driver jump path.
                          const entity =
                            c.entities && c.entities.length > 0
                              ? c.entities[0]
                              : undefined;
                          onJumpToSource(entity);
                          // SourceRef file is noted in the button tooltip;
                          // future refinement scrolls to (line, col).
                          void ref;
                        }
                      : undefined
                  }
                />
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
  jobId,
  notes,
  onInlineNotesChange,
}: {
  intent: import("@/types/artifact").IntentInput;
  jobId?: string;
  notes?: import("@/types/artifact").InlineNote[];
  onInlineNotesChange?: (next: import("@/types/artifact").InlineNote[]) => void;
}) {
  // IntentInput is Intent | string.
  const isStructured = typeof intent !== "string";
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2 shadow-sm">
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
                <li key={i} className="space-y-0.5">
                  <div className="flex items-baseline gap-2 text-[12px]">
                    <span className="text-[10px] font-mono text-muted-foreground tabular-nums w-5">
                      {i}.
                    </span>
                    <span className="text-[10px] font-mono uppercase text-muted-foreground w-[4.5rem]">
                      {c.evidence_type}
                    </span>
                    <span className="text-foreground">{c.statement}</span>
                  </div>
                  {jobId && onInlineNotesChange && (
                    <div className="pl-[5.75rem]">
                      <InlineNotes
                        jobId={jobId}
                        anchor={{ kind: "intent-claim", claim_index: i }}
                        notes={notes ?? []}
                        onChange={onInlineNotesChange}
                      />
                    </div>
                  )}
                </li>
              ))}
            </ol>
          )}
        </div>
      ) : (
        <div className="text-[12px] text-foreground max-h-72 overflow-y-auto">
          <MiniMarkdown source={intent} />
        </div>
      )}
    </section>
  );
}

/** Minimal markdown renderer — covers what PR descriptions actually
 *  use: headings, fenced code blocks, bullet + task lists, blank-
 *  line-separated paragraphs, and three inline marks (bold, italic,
 *  code). Everything else falls through as plain text. Ships without
 *  a markdown library; the bundle doesn't need a full parser for
 *  what a PR body throws at us. */
function MiniMarkdown({ source }: { source: string }) {
  const blocks = splitBlocks(source);
  return (
    <div className="space-y-2 leading-relaxed">
      {blocks.map((b, i) => {
        if (b.kind === "heading") {
          const cls =
            b.level === 1
              ? "text-[14px] font-semibold text-foreground"
              : b.level === 2
                ? "text-[13px] font-semibold text-foreground"
                : "text-[12px] font-semibold text-foreground";
          return (
            <p key={i} className={cls}>
              <InlineMd text={b.text} />
            </p>
          );
        }
        if (b.kind === "list") {
          // Plain bullets — rendered with a list-disc marker.
          return (
            <ul key={i} className="list-disc pl-5 space-y-0.5">
              {b.items.map((it, j) => (
                <li key={j}>
                  <InlineMd text={it} />
                </li>
              ))}
            </ul>
          );
        }
        if (b.kind === "tasks") {
          // Task list — compact checkboxes so [x]/[ ] reads as status.
          return (
            <ul key={i} className="pl-1 space-y-0.5">
              {b.items.map((it, j) => (
                <li key={j} className="flex items-baseline gap-2">
                  <span
                    aria-hidden
                    className={
                      "inline-flex items-center justify-center w-3 h-3 rounded-[3px] border text-[9px] font-mono translate-y-[1px] " +
                      (it.done
                        ? "border-emerald-500/60 bg-emerald-500/15 text-emerald-600 dark:text-emerald-300"
                        : "border-border/60 bg-background/60 text-transparent")
                    }
                  >
                    {it.done ? "✓" : ""}
                  </span>
                  <span className={it.done ? "text-muted-foreground" : ""}>
                    <InlineMd text={it.text} />
                  </span>
                </li>
              ))}
            </ul>
          );
        }
        if (b.kind === "code") {
          return (
            <pre
              key={i}
              className="rounded-md border border-border/60 bg-muted/30 px-3 py-2 overflow-x-auto text-[11px] font-mono leading-relaxed"
            >
              {b.lang && (
                <div className="text-[9px] font-mono uppercase tracking-wide text-muted-foreground mb-1">
                  {b.lang}
                </div>
              )}
              <code>{b.text}</code>
            </pre>
          );
        }
        return (
          <p key={i}>
            <InlineMd text={b.text} />
          </p>
        );
      })}
    </div>
  );
}

type MdBlock =
  | { kind: "paragraph"; text: string }
  | { kind: "heading"; level: number; text: string }
  | { kind: "list"; items: string[] }
  | { kind: "tasks"; items: { done: boolean; text: string }[] }
  | { kind: "code"; lang: string | null; text: string };

function splitBlocks(source: string): MdBlock[] {
  const out: MdBlock[] = [];
  const normalized = source.replace(/\r\n/g, "\n");
  // First extract fenced code blocks — they span multiple
  // paragraphs and blank-line splitting would shred them.
  const fenced: MdBlock[] = [];
  const sourceWithoutFences = normalized.replace(
    /```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g,
    (_, lang: string, body: string) => {
      fenced.push({
        kind: "code",
        lang: lang.trim() || null,
        text: body.replace(/\n+$/, ""),
      });
      return `\u0000FENCE_${fenced.length - 1}\u0000`;
    },
  );
  const chunks = sourceWithoutFences.split(/\n{2,}/);
  for (const raw of chunks) {
    const chunk = raw.trim();
    if (!chunk) continue;
    // Re-inject fenced blocks.
    const fence = /^\u0000FENCE_(\d+)\u0000$/.exec(chunk);
    if (fence) {
      out.push(fenced[Number(fence[1])]);
      continue;
    }
    const lines = chunk.split("\n");
    // Task list: every line begins with `- [ ]` or `- [x]`.
    if (lines.every((l) => /^[-*+]\s+\[[ xX]\]\s*/.test(l))) {
      out.push({
        kind: "tasks",
        items: lines.map((l) => {
          const m = l.match(/^[-*+]\s+\[([ xX])\]\s*(.*)$/);
          return {
            done: m !== null && m[1] !== " ",
            text: m ? m[2] : l,
          };
        }),
      });
      continue;
    }
    if (lines.every((l) => /^[-*+]\s+/.test(l))) {
      out.push({
        kind: "list",
        items: lines.map((l) => l.replace(/^[-*+]\s+/, "")),
      });
      continue;
    }
    const h = lines[0].match(/^(#{1,6})\s+(.*)$/);
    if (h && lines.length === 1) {
      out.push({ kind: "heading", level: h[1].length, text: h[2] });
      continue;
    }
    out.push({ kind: "paragraph", text: chunk });
  }
  return out;
}

/** Render inline bold, italic, and code spans. Unmatched marks fall
 *  back to literal text. */
function InlineMd({ text }: { text: string }) {
  const parts: import("react").ReactNode[] = [];
  // Greedy single pass: backticks first so code spans don't get
  // italicised, then bold (**x**), then italic (*x*).
  const re = /(`[^`]+`|\*\*[^*]+\*\*|\*[^*]+\*)/g;
  let lastIndex = 0;
  for (const m of text.matchAll(re)) {
    const idx = m.index ?? 0;
    if (idx > lastIndex) parts.push(text.slice(lastIndex, idx));
    const token = m[0];
    if (token.startsWith("`")) {
      parts.push(
        <code
          key={`c-${idx}`}
          className="font-mono text-[11px] px-1 py-[1px] rounded bg-muted/60 text-foreground"
        >
          {token.slice(1, -1)}
        </code>,
      );
    } else if (token.startsWith("**")) {
      parts.push(
        <strong key={`b-${idx}`} className="font-semibold text-foreground">
          {token.slice(2, -2)}
        </strong>,
      );
    } else {
      parts.push(
        <em key={`i-${idx}`} className="italic">
          {token.slice(1, -1)}
        </em>,
      );
    }
    lastIndex = idx + token.length;
  }
  if (lastIndex < text.length) parts.push(text.slice(lastIndex));
  return <>{parts}</>;
}

function IntentFitCard({
  fit,
}: {
  fit: import("@/types/artifact").IntentFit | null;
}) {
  return (
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2.5 shadow-sm">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Intent fit
        </h2>
        {/* Model stamp lives in the artifact-wide BaselineStamp /
            drift banner, not on every card — keeps model names out
            of the hot copy. Tooltip on the verdict pill has the
            diagnostic details for a reviewer who wants them. */}
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
    <section className="rounded border border-border/60 bg-muted/10 px-4 py-3 space-y-2.5 shadow-sm">
      <div className="flex items-baseline justify-between">
        <h2 className="text-[11px] font-medium text-muted-foreground tracking-wide uppercase">
          Proof
        </h2>
        {/* Model stamp lives in the artifact-wide BaselineStamp /
            drift banner, not on every card. */}
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
