import { useEffect, useState } from "react";

/** One-time dismissable hint.
 *
 *  Shown to a first-time reviewer on a PR workspace to demystify the
 *  three signals (Intent-fit · Proof · Nav cost) before they hit
 *  them scattered across tabs. localStorage-gated per `key` so a
 *  dismissed hint stays dismissed across sessions and repeat visits.
 *
 *  Keep the surface small — a full tour would have pulled focus
 *  away from the actual PR; one thin strip the reviewer can close
 *  with a single click is enough nudge without overstaying. */
export function FirstRunHint({
  storageKey,
  title,
  children,
}: {
  storageKey: string;
  title: string;
  children: React.ReactNode;
}) {
  const fullKey = `adr.hint.${storageKey}.seen`;
  const [visible, setVisible] = useState(() => {
    try {
      return localStorage.getItem(fullKey) !== "1";
    } catch {
      return true;
    }
  });
  useEffect(() => {
    // Re-read on mount: a second tab could have dismissed already.
    try {
      if (localStorage.getItem(fullKey) === "1") setVisible(false);
    } catch {
      /* ignore */
    }
  }, [fullKey]);
  if (!visible) return null;
  const dismiss = () => {
    try {
      localStorage.setItem(fullKey, "1");
    } catch {
      /* ignore */
    }
    setVisible(false);
  };
  return (
    <section className="rounded-md border border-dashed border-border/70 bg-muted/30 px-3 py-2.5 space-y-1.5 text-[11px] text-foreground">
      <header className="flex items-baseline justify-between gap-2">
        <p className="font-semibold text-[12px]">{title}</p>
        <button
          type="button"
          onClick={dismiss}
          className="text-[10px] font-mono text-muted-foreground hover:text-foreground"
          aria-label="dismiss hint"
        >
          got it ×
        </button>
      </header>
      <div className="text-[11px] leading-relaxed text-muted-foreground">
        {children}
      </div>
    </section>
  );
}
