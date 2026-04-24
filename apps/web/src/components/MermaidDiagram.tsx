import { useEffect, useRef, useState } from "react";
import DOMPurify from "dompurify";
import { useTheme } from "@/lib/theme";

/** Render a Mermaid spec into inline SVG.
 *
 *  Mermaid is loaded lazily (dynamic import) so the ~800 KB runtime
 *  only ships when a flow actually carries a diagram. Re-renders when
 *  the spec or theme changes.
 *
 *  LLM-provided Mermaid source is untrusted. We run Mermaid with
 *  `securityLevel: "strict"` and then parse the resulting SVG with
 *  DOMPurify (SVG profile) which returns a DOM Node we append with
 *  standard DOM methods — no raw `innerHTML` path for LLM content.
 *
 *  On parse failure we surface a discreet error block instead of
 *  crashing — LLM output is noisy, don't let one bad spec poison the
 *  whole Flow tab. */
export function MermaidDiagram({
  source,
  label,
}: {
  source: string;
  label?: string;
}) {
  const hostRef = useRef<HTMLDivElement | null>(null);
  const [err, setErr] = useState<string | null>(null);
  const [theme] = useTheme();
  const idRef = useRef<string>(
    `mermaid-${Math.random().toString(36).slice(2, 10)}`,
  );

  useEffect(() => {
    let cancelled = false;
    setErr(null);
    (async () => {
      try {
        const mod = await import("mermaid");
        const mermaid = mod.default;
        mermaid.initialize({
          startOnLoad: false,
          theme: theme === "dark" ? "dark" : "default",
          securityLevel: "strict",
          fontFamily: "inherit",
        });
        const { svg } = await mermaid.render(idRef.current, source);
        if (cancelled || !hostRef.current) return;
        // DOMPurify returns a TrustedHTML-compatible NODE when we
        // pass `RETURN_DOM_FRAGMENT: true`. We then append the
        // fragment via DOM methods — no innerHTML assignment of any
        // untrusted string.
        const frag = DOMPurify.sanitize(svg, {
          USE_PROFILES: { svg: true, svgFilters: true },
          RETURN_DOM_FRAGMENT: true,
        }) as DocumentFragment;
        while (hostRef.current.firstChild) {
          hostRef.current.removeChild(hostRef.current.firstChild);
        }
        hostRef.current.appendChild(frag);
      } catch (e) {
        if (!cancelled) setErr(String(e));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, [source, theme]);

  if (err) {
    return (
      <div className="rounded-md border border-amber-400/40 bg-amber-500/5 px-3 py-2 text-[11px] font-mono text-amber-800 dark:text-amber-200 space-y-1">
        <div>Mermaid parse failed — showing raw source.</div>
        <pre className="whitespace-pre-wrap break-words text-[10px] text-muted-foreground">
          {source}
        </pre>
      </div>
    );
  }
  return (
    <figure className="rounded-md border border-border/60 bg-muted/40 shadow-sm p-3">
      {label && (
        <figcaption className="text-[10px] font-mono uppercase tracking-wider text-muted-foreground mb-2">
          {label}
        </figcaption>
      )}
      <div
        ref={hostRef}
        className="overflow-x-auto [&_svg]:max-w-full [&_svg]:h-auto"
      />
    </figure>
  );
}
