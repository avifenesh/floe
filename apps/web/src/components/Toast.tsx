import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";

/** Minimal toast system to replace `alert()` calls. Supports:
 *  - tone (info / success / warn / error)
 *  - optional recover action (label + handler)
 *  - auto-dismiss after N ms (off by default for error)
 *
 *  Mount `<ToastProvider>` once near the root; call `useToast().push()`
 *  from anywhere. Shape kept tight — this is about displacing `alert()`,
 *  not becoming a framework.
 */

export type ToastTone = "info" | "success" | "warn" | "error";

export interface ToastAction {
  label: string;
  onClick: () => void;
}

export interface ToastInput {
  title: string;
  body?: string;
  tone?: ToastTone;
  action?: ToastAction;
  /** Auto-dismiss after ms. Defaults: info/success 4000, warn 6000,
   *  error 0 (sticky until dismissed). */
  ttlMs?: number;
}

interface ToastEntry extends ToastInput {
  id: number;
  tone: ToastTone;
}

interface ToastCtx {
  push: (t: ToastInput) => number;
  dismiss: (id: number) => void;
}

const Ctx = createContext<ToastCtx | null>(null);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [toasts, setToasts] = useState<ToastEntry[]>([]);
  const next = useRef(1);

  const dismiss = useCallback((id: number) => {
    setToasts((prev) => prev.filter((t) => t.id !== id));
  }, []);

  const push = useCallback((t: ToastInput) => {
    const id = next.current++;
    const tone = t.tone ?? "info";
    const entry: ToastEntry = { ...t, id, tone };
    setToasts((prev) => [...prev, entry]);
    const ttl =
      t.ttlMs ?? (tone === "error" ? 0 : tone === "warn" ? 6000 : 4000);
    if (ttl > 0) {
      setTimeout(() => dismiss(id), ttl);
    }
    return id;
  }, [dismiss]);

  const value = useMemo<ToastCtx>(() => ({ push, dismiss }), [push, dismiss]);

  return (
    <Ctx.Provider value={value}>
      {children}
      <div className="fixed top-4 right-4 z-50 flex flex-col gap-2 max-w-sm pointer-events-none">
        {toasts.map((t) => (
          <ToastCard key={t.id} t={t} onClose={() => dismiss(t.id)} />
        ))}
      </div>
    </Ctx.Provider>
  );
}

export function useToast(): ToastCtx {
  const ctx = useContext(Ctx);
  if (!ctx) {
    throw new Error("useToast must be used inside <ToastProvider>");
  }
  return ctx;
}

function toneClass(tone: ToastTone): string {
  switch (tone) {
    case "success":
      return "border-emerald-400/60 bg-emerald-50 text-emerald-900 dark:bg-emerald-400/10 dark:text-emerald-100";
    case "warn":
      return "border-amber-400/60 bg-amber-50 text-amber-900 dark:bg-amber-400/10 dark:text-amber-100";
    case "error":
      return "border-rose-400/60 bg-rose-50 text-rose-900 dark:bg-rose-400/10 dark:text-rose-100";
    case "info":
    default:
      return "border-border/60 bg-background text-foreground";
  }
}

function ToastCard({ t, onClose }: { t: ToastEntry; onClose: () => void }) {
  // Enter animation — mount with opacity 0 / translate, then flip.
  const [shown, setShown] = useState(false);
  useEffect(() => {
    const id = requestAnimationFrame(() => setShown(true));
    return () => cancelAnimationFrame(id);
  }, []);
  return (
    <div
      role="status"
      aria-live="polite"
      className={
        "pointer-events-auto rounded-md border px-3 py-2.5 shadow-sm space-y-1.5 transition-all duration-200 " +
        toneClass(t.tone) +
        (shown ? " opacity-100 translate-x-0" : " opacity-0 translate-x-4")
      }
    >
      <div className="flex items-baseline justify-between gap-2">
        <p className="text-[12px] font-semibold leading-tight">{t.title}</p>
        <button
          type="button"
          onClick={onClose}
          aria-label="dismiss"
          className="text-[14px] leading-none opacity-60 hover:opacity-100 -mr-1"
        >
          ×
        </button>
      </div>
      {t.body && (
        <p className="text-[11px] leading-relaxed font-mono opacity-85 whitespace-pre-wrap break-words">
          {t.body}
        </p>
      )}
      {t.action && (
        <div className="pt-0.5">
          <button
            type="button"
            onClick={() => {
              t.action!.onClick();
              onClose();
            }}
            className="text-[11px] font-mono underline underline-offset-2 decoration-dotted hover:decoration-solid"
          >
            {t.action.label}
          </button>
        </div>
      )}
    </div>
  );
}
