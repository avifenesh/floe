/** Landing-page hero. Sits above the samples gallery to teach a
 *  first-time visitor what the product is in ~5 seconds.
 *
 *  Two panels side by side (stacked on mobile):
 *  - Tagline + three-bullet pitch.
 *  - A sample flow card — shows what a real analysis looks like
 *    (intent-fit verdict, proof chip, cost bar) without needing to
 *    run one. This is the product explaining itself visually.
 *
 *  CTA row: Sign in with GitHub (primary) + "Try a sample" which
 *  scrolls the samples gallery into view.
 */

interface Props {
  signedIn: boolean;
  githubLoginUrl: string;
  onTrySample: () => void;
}

export function LandingHero({ signedIn, githubLoginUrl, onTrySample }: Props) {
  return (
    <section className="rounded-2xl border border-border/60 bg-muted/10 overflow-hidden">
      <div className="grid grid-cols-1 md:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
        {/* Left: pitch */}
        <div className="p-6 md:p-8 space-y-5 min-w-0">
          <div className="space-y-2">
            <p className="text-[11px] font-mono uppercase tracking-wider text-muted-foreground">
              Architectural PR review · TypeScript
            </p>
            <h1 className="text-[22px] md:text-[26px] font-semibold text-foreground leading-tight">
              PRs aren&apos;t diffs. They&apos;re{" "}
              <span className="underline decoration-muted-foreground/40 underline-offset-4">
                stories
              </span>
              .
            </h1>
            <p className="text-[13px] text-muted-foreground leading-relaxed">
              We turn a TypeScript PR into flows — one per architectural story —
              and tell you three things per flow that <code>git diff</code> can&apos;t:
            </p>
          </div>
          <ul className="space-y-2 text-[13px] text-foreground">
            <PitchBullet
              kind="fit"
              title="Intent-fit"
              body="Does this flow actually deliver something the PR's stated intent claims? Delivers / partial / unrelated, with the matching claim cited."
            />
            <PitchBullet
              kind="proof"
              title="Proof"
              body="Is there real evidence — a benchmark log, an examples/ file, a claim-asserting test? Unit-test presence is not proof."
            />
            <PitchBullet
              kind="cost"
              title="Nav cost"
              body="Signed delta of how hard the next LLM session has to work to navigate the affected flow. Refactors go negative."
            />
          </ul>
          <div className="flex flex-wrap items-center gap-2 pt-2">
            {!signedIn && (
              <a
                href={githubLoginUrl}
                className="inline-flex items-center gap-2 text-[13px] font-medium rounded-md border border-foreground/80 bg-foreground text-background px-4 py-2 hover:bg-foreground/90 transition-colors"
              >
                <GithubGlyph />
                <span>Sign in with GitHub</span>
              </a>
            )}
            <button
              onClick={onTrySample}
              className="inline-flex items-center gap-2 text-[13px] font-medium rounded-md border border-border/60 bg-background px-4 py-2 hover:bg-muted transition-colors"
            >
              <span aria-hidden>▼</span>
              <span>Try a sample</span>
            </button>
          </div>
        </div>

        {/* Right: sample PR preview — stack of three mini flow rows so
            the column actually fills its share (was one card in a
            ~340px panel, looked half-empty). Shows the product's real
            output shape: multiple flows, varied verdicts, cost deltas. */}
        <div className="hidden md:flex md:flex-col border-t md:border-t-0 md:border-l border-border/60 bg-muted/5 p-6 md:p-8 gap-4 min-w-0">
          <div className="flex items-baseline justify-between">
            <p className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground">
              Sample output · PR #181
            </p>
            <p className="text-[10px] font-mono text-muted-foreground">
              5 flows · 3 axes
            </p>
          </div>
          <div className="space-y-2 flex-1">
            <MiniFlowCard
              name="Streaming chunk API"
              rationale="Adds Job.streamChunk — chunk-level streaming primitive."
              fit="delivers"
              proof="strong"
              axes={[
                { key: "runtime", net: -19, pct: -4.9 },
                { key: "continuation", net: 6, pct: 0.2 },
              ]}
              verified="src/job.ts:303-307 · bench in notes"
            />
            <MiniFlowCard
              name="Queue budget"
              rationale="Adds per-category maxTokens + maxCosts on BudgetOptions."
              fit="delivers"
              proof="partial"
              axes={[
                { key: "operational", net: -8, pct: -1.1 },
                { key: "runtime", net: 3, pct: 0.3 },
              ]}
              verified="src/types.ts:575 · tests/budget.test.ts"
            />
            <MiniFlowCard
              name="TestJob / TestQueue scaffolding"
              rationale="Testing-mode variants of the new streaming API."
              fit="partial"
              proof="missing"
              axes={[
                { key: "runtime", net: 3, pct: 0.3 },
                { key: "continuation", net: 3, pct: 0.1 },
              ]}
              verified=""
            />
          </div>
        </div>
      </div>
    </section>
  );
}

function PitchBullet({
  kind,
  title,
  body,
}: {
  kind: "fit" | "proof" | "cost";
  title: string;
  body: string;
}) {
  const glyph = kind === "fit" ? "◆" : kind === "proof" ? "✓" : "Δ";
  return (
    <li className="flex items-baseline gap-3">
      <span
        className="inline-flex items-center justify-center w-5 h-5 rounded bg-muted-foreground/10 text-muted-foreground text-[11px] font-mono shrink-0"
        aria-hidden
      >
        {glyph}
      </span>
      <span className="leading-relaxed">
        <span className="font-semibold">{title}.</span>{" "}
        <span className="text-muted-foreground">{body}</span>
      </span>
    </li>
  );
}

/** Chip with a semantic tone. `ok` = neutral confidence, `warn` =
 *  partial, `miss` = missing/low. Themed so it reads on both light
 *  and dark backgrounds. */
function Chip({
  label,
  tone,
}: {
  label: string;
  tone: "ok" | "warn" | "miss";
}) {
  const cls =
    tone === "ok"
      ? "border-border/60 bg-muted/40 text-foreground"
      : tone === "warn"
        ? "border-amber-400/40 bg-amber-100/50 dark:bg-amber-400/10 text-amber-900 dark:text-amber-200"
        : "border-destructive/30 bg-destructive/10 text-destructive";
  return (
    <span
      className={`text-[9px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border ${cls}`}
    >
      {label}
    </span>
  );
}

/** Tiny axis bar — centred zero, fill proportional to |pct|, cap 50%
 *  so the half-width stays proportional on the card. */
function AxisBar({ pct }: { pct: number }) {
  const width = Math.min(50, Math.abs(pct) * 5);
  const left = pct < 0 ? 50 - width : 50;
  return (
    <div className="h-[3px] rounded-full bg-muted overflow-hidden relative">
      <div
        className="absolute top-0 h-full bg-muted-foreground/50 rounded-full"
        style={{ left: `${left}%`, width: `${width}%` }}
      />
    </div>
  );
}

interface MiniFlowProps {
  name: string;
  rationale: string;
  fit: "delivers" | "partial" | "unrelated";
  proof: "strong" | "partial" | "missing";
  axes: Array<{ key: string; net: number; pct: number }>;
  verified: string;
}

/** One row in the sample-PR preview. Compact enough that three fit
 *  vertically inside the hero's right column, each showing the
 *  headline view a reviewer sees on a real analysis: flow name,
 *  one-line rationale, FIT/PROOF chips, two axes with signed percent,
 *  and the cited evidence. */
function MiniFlowCard({
  name,
  rationale,
  fit,
  proof,
  axes,
  verified,
}: MiniFlowProps) {
  const fitTone: "ok" | "warn" | "miss" =
    fit === "delivers" ? "ok" : fit === "partial" ? "warn" : "miss";
  const proofTone: "ok" | "warn" | "miss" =
    proof === "strong" ? "ok" : proof === "partial" ? "warn" : "miss";
  return (
    <article className="rounded-lg border border-border/60 bg-background/40 p-3 space-y-2">
      <header className="space-y-0.5">
        <p className="text-[12px] font-mono font-semibold text-foreground leading-tight">
          {name}
        </p>
        <p className="text-[10px] text-muted-foreground leading-snug">
          {rationale}
        </p>
      </header>
      <div className="flex flex-wrap gap-1">
        <Chip label={`FIT: ${fit}`} tone={fitTone} />
        <Chip label={`PROOF: ${proof}`} tone={proofTone} />
      </div>
      <div className="space-y-1">
        {axes.map((a) => (
          <div key={a.key} className="space-y-0.5">
            <div className="flex items-baseline justify-between text-[10px] font-mono text-muted-foreground">
              <span>{a.key}</span>
              <span className="tabular-nums text-foreground">
                {a.net > 0 ? "+" : a.net < 0 ? "\u2212" : ""}
                {Math.abs(a.net)} ({a.pct > 0 ? "+" : a.pct < 0 ? "\u2212" : ""}
                {Math.abs(a.pct).toFixed(1)}%)
              </span>
            </div>
            <AxisBar pct={a.pct} />
          </div>
        ))}
      </div>
      {verified && (
        <p className="text-[10px] font-mono text-muted-foreground pt-1 border-t border-border/40 leading-snug">
          verified: {verified}
        </p>
      )}
    </article>
  );
}

function GithubGlyph() {
  return (
    <svg
      width="14"
      height="14"
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden
    >
      <path d="M8 0a8 8 0 0 0-2.53 15.59c.4.07.55-.17.55-.38v-1.34c-2.23.48-2.7-1.08-2.7-1.08-.36-.92-.89-1.17-.89-1.17-.73-.5.06-.49.06-.49.8.06 1.22.83 1.22.83.72 1.22 1.88.87 2.34.66.07-.52.28-.87.5-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.01.08-2.12 0 0 .67-.21 2.2.82a7.7 7.7 0 0 1 4 0c1.53-1.03 2.2-.82 2.2-.82.44 1.11.16 1.92.08 2.12.51.56.82 1.28.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48v2.2c0 .21.15.46.55.38A8 8 0 0 0 8 0Z" />
    </svg>
  );
}
