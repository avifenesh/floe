import { useEffect, useState } from "react";

/** Three-step onboarding walkthrough for the PR workspace.
 *
 *  Shown once per user via localStorage. Steps:
 *    1. Flows — the PR broken into architectural stories
 *    2. Intent & Proof — does each flow deliver what the PR claims?
 *    3. Nav cost — signed delta of reviewer/LLM navigation effort
 *
 *  Replaces the static single-card `FirstRunHint` with a paged
 *  walkthrough so the reviewer can step through the three signals
 *  rather than read them all at once. Each step cites the tab/view
 *  the signal lives on so the reviewer knows where to look.
 *
 *  Dismissable at any step via the × or "skip". Reaches the end
 *  when "got it" is clicked on step 3.
 */
export function OnboardingTour({ storageKey }: { storageKey: string }) {
  const fullKey = `adr.tour.${storageKey}.done`;
  const [visible, setVisible] = useState(() => {
    try {
      return localStorage.getItem(fullKey) !== "1";
    } catch {
      return true;
    }
  });
  const [step, setStep] = useState(0);
  useEffect(() => {
    try {
      if (localStorage.getItem(fullKey) === "1") setVisible(false);
    } catch {
      /* ignore */
    }
  }, [fullKey]);
  if (!visible) return null;
  const finish = () => {
    try {
      localStorage.setItem(fullKey, "1");
    } catch {
      /* ignore */
    }
    setVisible(false);
  };
  const steps: Array<{ title: string; where: string; body: React.ReactNode }> = [
    {
      title: "Flows — architectural stories",
      where: "the PR tab and per-flow tabs",
      body: (
        <>
          We break the diff into flows, one per architectural story. Each flow
          is self-contained — same shape the reviewer already thinks in.
        </>
      ),
    },
    {
      title: "Intent & Proof",
      where: "the Intent & Proof sub-tab",
      body: (
        <>
          Intent-fit asks: does this flow deliver something the PR claims?{" "}
          Proof asks: is there evidence for it? Unit-test presence is not
          proof — we look for benchmarks, examples, claim-asserting tests.
        </>
      ),
    },
    {
      title: "Nav cost",
      where: "the Cost sub-tab",
      body: (
        <>
          A signed delta of how hard the next LLM session has to work on the
          affected flow. Refactors go negative (easier). Bars scale as a
          percentage of the per-repo baseline, not relative rank.
        </>
      ),
    },
  ];
  const s = steps[step] ?? steps[0];
  const last = step === steps.length - 1;
  return (
    <section className="rounded-md border border-dashed border-border/70 bg-muted/40 px-3.5 py-3 space-y-2 text-[11px] text-foreground shadow-sm">
      <header className="flex items-baseline justify-between gap-2">
        <p className="font-semibold text-[12px]">{s.title}</p>
        <div className="flex items-baseline gap-3">
          <span className="text-[10px] font-mono text-muted-foreground">
            {step + 1} / {steps.length}
          </span>
          <button
            type="button"
            onClick={finish}
            className="text-[10px] font-mono text-muted-foreground hover:text-foreground"
            aria-label="skip onboarding"
          >
            skip ×
          </button>
        </div>
      </header>
      <p className="text-[11px] leading-relaxed text-muted-foreground">
        <span className="text-foreground/80">{s.body}</span>{" "}
        <span className="opacity-70">Find this on {s.where}.</span>
      </p>
      <div className="flex items-center gap-2 pt-1">
        <div className="flex gap-1" aria-hidden>
          {steps.map((_, i) => (
            <span
              key={i}
              className={
                "inline-block w-1.5 h-1.5 rounded-full " +
                (i === step ? "bg-foreground" : "bg-muted-foreground/30")
              }
            />
          ))}
        </div>
        <div className="ml-auto flex items-center gap-1.5">
          {step > 0 && (
            <button
              type="button"
              onClick={() => setStep((x) => Math.max(0, x - 1))}
              className="text-[11px] font-mono rounded border border-border/60 bg-background hover:bg-muted/40 px-2 py-0.5 transition-colors"
            >
              back
            </button>
          )}
          <button
            type="button"
            onClick={() =>
              last ? finish() : setStep((x) => Math.min(steps.length - 1, x + 1))
            }
            className="text-[11px] font-mono rounded border border-foreground/70 bg-foreground text-background hover:bg-foreground/90 px-2.5 py-0.5 transition-colors"
          >
            {last ? "got it" : "next →"}
          </button>
        </div>
      </div>
    </section>
  );
}
