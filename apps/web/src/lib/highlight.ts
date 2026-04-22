import { createHighlighterCore, type HighlighterCore } from "shiki/core";
import { createJavaScriptRegexEngine } from "shiki/engine/javascript";

export type Lang = "typescript" | "tsx";
export type Token = { content: string; color?: string };
export type HighlightedLines = Token[][];

let instance: Promise<HighlighterCore> | null = null;

/** Lazy singleton. Loads only the TypeScript + TSX grammars and the
 *  two GitHub themes the product actually renders — via Shiki's
 *  fine-grained core entry, not the default bundle.
 *
 *  The default `shiki` entry pulls every language grammar (~200 of
 *  them) and the full Oniguruma WASM engine, landing ~10 MB of chunks
 *  we don't use. `shiki/core` + dynamic-imported langs + the JS regex
 *  engine (good enough for TS/TSX) cuts that to roughly one index
 *  chunk under 500 KB. */
function highlighter(): Promise<HighlighterCore> {
  if (!instance) {
    instance = createHighlighterCore({
      themes: [
        import("@shikijs/themes/github-light"),
        import("@shikijs/themes/github-dark"),
      ],
      langs: [
        import("@shikijs/langs/typescript"),
        import("@shikijs/langs/tsx"),
      ],
      engine: createJavaScriptRegexEngine(),
    });
  }
  return instance;
}

/** Tokenize `code` with the requested theme. Returns one Token[] per line. */
export async function highlight(
  code: string,
  lang: Lang,
  theme: "light" | "dark",
): Promise<HighlightedLines> {
  const hl = await highlighter();
  const themeKey = theme === "dark" ? "github-dark" : "github-light";
  const result = hl.codeToTokens(code, { lang, theme: themeKey });
  return result.tokens.map((line) => line.map((t) => ({ content: t.content, color: t.color })));
}

/** Derive the grammar from a path extension. Non-TS paths fall back to
 *  `typescript` — the highlighter is a safe superset for diff reading. */
export function langForPath(path: string): Lang {
  return path.endsWith(".tsx") ? "tsx" : "typescript";
}
