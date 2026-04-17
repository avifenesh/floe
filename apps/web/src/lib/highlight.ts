import { createHighlighter, type Highlighter } from "shiki";

export type Lang = "typescript" | "tsx";
export type Token = { content: string; color?: string };
export type HighlightedLines = Token[][];

let instance: Promise<Highlighter> | null = null;

/** Lazy singleton. Loads typescript + tsx grammars and github themes
 *  on first use; subsequent calls reuse the same highlighter. */
function highlighter(): Promise<Highlighter> {
  if (!instance) {
    instance = createHighlighter({
      themes: ["github-light", "github-dark"],
      langs: ["typescript", "tsx"],
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
