/** Landing-page demo gallery — loads /samples from the backend and
 *  renders one clickable card per built-in PR.
 *
 *  Each card fires `onPick(id)`, which the parent wires to
 *  `analyzeSample` + pipeline polling. The gallery renders nothing
 *  when the server has no samples (bare-bones deploy without the
 *  repo's `fixtures/` dir), so there's no empty state to explain.
 */

import { useEffect, useState } from "react";
import { fetchSamples, type SampleView } from "@/api";

interface Props {
  onPick: (sampleId: string) => void;
  /** Optional: disable all cards while one is spinning up. */
  disabled?: boolean;
  /** Optional: the id of the sample currently being analysed — gets
   *  a "loading" treatment so the reviewer sees which they clicked. */
  activeId?: string | null;
}

export function SamplesGallery({ onPick, disabled, activeId }: Props) {
  const [samples, setSamples] = useState<SampleView[] | null>(null);
  const [err, setErr] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    fetchSamples()
      .then((s) => {
        if (!cancelled) setSamples(s);
      })
      .catch((e) => {
        if (!cancelled) setErr(String(e));
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Server unreachable → silent. Absence of gallery is an honest
  // signal; spraying an error across the landing for a reviewer who
  // just wants to read the pitch would be worse.
  if (err) return null;

  // Not loaded yet → terse placeholder so the layout doesn't jump
  // when the cards land. Same height as a small card strip.
  if (samples === null) {
    return (
      <section className="space-y-3">
        <SectionHeader />
        <div className="h-24 rounded-lg border border-dashed border-border/40 bg-muted/5" />
      </section>
    );
  }

  if (samples.length === 0) {
    return null;
  }

  return (
    <section className="space-y-3">
      <SectionHeader count={samples.length} />
      <ul className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
        {samples.map((s) => (
          <li key={s.id}>
            <SampleCard
              sample={s}
              onPick={() => onPick(s.id)}
              disabled={disabled ?? false}
              loading={activeId === s.id}
            />
          </li>
        ))}
      </ul>
    </section>
  );
}

function SectionHeader({ count }: { count?: number }) {
  return (
    <div className="flex items-baseline gap-2">
      <h2 className="text-[13px] font-medium text-foreground">
        Try a sample
      </h2>
      {count !== undefined && (
        <span className="text-[11px] font-mono text-muted-foreground">
          {count} PR{count === 1 ? "" : "s"}
        </span>
      )}
      <span className="text-[11px] text-muted-foreground ml-auto">
        Runs on the server — no sign-in required.
      </span>
    </div>
  );
}

function SampleCard({
  sample,
  onPick,
  disabled,
  loading,
}: {
  sample: SampleView;
  onPick: () => void;
  disabled: boolean;
  loading: boolean;
}) {
  const busy = loading || disabled;
  return (
    <button
      type="button"
      onClick={onPick}
      disabled={busy}
      className={
        "w-full text-left rounded-lg border border-border/60 bg-background hover:bg-muted/20 " +
        "px-3 py-2.5 transition-colors disabled:cursor-not-allowed disabled:opacity-60 " +
        (loading ? "ring-2 ring-foreground/40" : "")
      }
    >
      <div className="flex items-baseline gap-2">
        <span className="text-[13px] font-medium text-foreground truncate">
          {sample.title}
        </span>
        {loading && (
          <span
            className="text-[10px] font-mono text-muted-foreground ml-auto"
            aria-live="polite"
          >
            analysing…
          </span>
        )}
      </div>
      <p className="mt-1 text-[11px] text-muted-foreground leading-relaxed line-clamp-2">
        {sample.description}
      </p>
      <p className="mt-1.5 text-[10px] font-mono text-muted-foreground/70">
        {sample.id}
      </p>
    </button>
  );
}
