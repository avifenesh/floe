import { useEffect, useState } from "react";

/** Keyboard cheatsheet — pops on `?`, closes on Esc or backdrop click.
 *
 *  Listed shortcuts match the bindings the product already handles:
 *  `/` for the palette, `Esc` to close overlays. Intentionally short
 *  — long cheatsheets go unread. */
const SHORTCUTS: Array<{ keys: string[]; label: string }> = [
  { keys: ["/"], label: "open command palette" },
  { keys: ["?"], label: "show this help" },
  { keys: ["Esc"], label: "close overlay" },
];

export function KeyHelp() {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "?" && !isTyping(e.target)) {
        e.preventDefault();
        setOpen((o) => !o);
      } else if (e.key === "Escape" && open) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);
  if (!open) return null;
  return (
    <div
      className="fixed inset-0 z-40 bg-background/60 backdrop-blur-[2px] flex items-center justify-center"
      onClick={() => setOpen(false)}
      role="dialog"
      aria-label="keyboard shortcuts"
    >
      <div
        className="rounded-md border border-border/70 bg-background shadow-lg w-[360px] max-w-[92vw] p-4 space-y-3"
        onClick={(e) => e.stopPropagation()}
      >
        <header className="flex items-baseline justify-between">
          <h2 className="text-[13px] font-semibold">Keyboard</h2>
          <button
            onClick={() => setOpen(false)}
            className="text-[14px] leading-none text-muted-foreground hover:text-foreground"
            aria-label="close"
          >
            ×
          </button>
        </header>
        <ul className="space-y-1.5">
          {SHORTCUTS.map((s) => (
            <li key={s.label} className="flex items-baseline justify-between gap-3">
              <span className="text-[12px] text-foreground">{s.label}</span>
              <span className="flex items-center gap-1">
                {s.keys.map((k) => (
                  <kbd
                    key={k}
                    className="rounded border border-border/70 bg-muted/50 px-1.5 py-0.5 text-[10px] font-mono"
                  >
                    {k}
                  </kbd>
                ))}
              </span>
            </li>
          ))}
        </ul>
      </div>
    </div>
  );
}

function isTyping(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  const tag = target.tagName.toLowerCase();
  return tag === "input" || tag === "textarea" || target.isContentEditable;
}
