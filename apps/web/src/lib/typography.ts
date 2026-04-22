/** Typography tokens — one source of truth for text styles.
 *
 *  Rule of thumb:
 *  - **Sans** for prose, headings, copy, labels, error messages.
 *  - **Mono** for data: numbers, hashes, identifiers, file paths,
 *    code spans, status pill values, metadata stamps.
 *  - **tabular-nums** on every numeric span so digits don't dance
 *    when values change.
 *
 *  Apply by spreading the constant into a className:
 *    <h2 className={T.sectionEyebrow}>Per-flow verdicts</h2>
 *
 *  When a component needs a deviation, compose: `${T.body} text-rose-500`.
 *  Don't reach past tokens for routine cases — outliers fragment the
 *  system and the next person can't tell what's intentional.
 */

export const T = {
  // ── Headings ────────────────────────────────────────────────────────
  /** Page-level greeting / single H1 per route. */
  h1: "text-[18px] font-semibold text-foreground leading-tight",
  /** Tab / panel sub-heading inside a workspace view. */
  h2: "text-[13px] font-mono text-foreground",
  /** UPPERCASE eyebrow label above a list or section.
   *  Used for "Per-flow verdicts", "Recent PRs", "Drivers", etc. */
  sectionEyebrow:
    "text-[11px] font-medium text-muted-foreground tracking-wide uppercase",

  // ── Body & copy ─────────────────────────────────────────────────────
  /** Default paragraph copy in workspace views. */
  body: "text-[12px] text-muted-foreground leading-relaxed",
  /** Tighter variant for hint text under a control. */
  hint: "text-[11px] text-muted-foreground leading-snug",
  /** Inline metadata next to a heading (model stamp, count, etc.). */
  meta: "text-[10px] font-mono text-muted-foreground",

  // ── Hero numbers ────────────────────────────────────────────────────
  /** The 22-px headline number (e.g. cost %). Pair with a color
   *  helper from `verdict-color` for sign tinting. */
  hero: "text-[22px] font-mono font-semibold tabular-nums leading-none",
  /** Subtext beside the hero — same digit family, smaller. */
  heroSub: "text-[14px] font-mono tabular-nums text-muted-foreground",

  // ── Data identifiers ────────────────────────────────────────────────
  /** Function names, file paths, hashes, qualified identifiers. */
  ident: "text-[12px] font-mono text-foreground",
  /** Short inline code (env var, flag). */
  codeInline: "rounded bg-muted/50 px-1 text-[11px] font-mono",

  // ── Interactive ─────────────────────────────────────────────────────
  /** Primary CTA — Analyse, Sign in, Submit. */
  btnPrimary:
    "text-[13px] font-medium rounded-md border border-foreground/80 bg-foreground text-background px-4 py-2 hover:bg-foreground/90 disabled:opacity-40 disabled:cursor-not-allowed transition-colors",
  /** Inline utility link — refresh, dismiss, expand, sign out. */
  btnUtility:
    "text-[10px] font-mono text-muted-foreground hover:text-foreground transition-colors",

  // ── Status pills (color tone applied separately, see verdict-color) ─
  /** Small verdict / status chip — pair with a color class. */
  pill: "text-[10px] font-mono uppercase tracking-wide px-1.5 py-0.5 rounded-full border",
};
