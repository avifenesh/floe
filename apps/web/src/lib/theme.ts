import { useEffect, useState } from "react";

export type Theme = "light" | "dark";

const KEY = "adr.theme";

/** Initial theme: explicit user choice wins, else system preference, else light. */
function initial(): Theme {
  if (typeof window === "undefined") return "light";
  const stored = localStorage.getItem(KEY);
  if (stored === "light" || stored === "dark") return stored;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function apply(t: Theme) {
  const el = document.documentElement;
  el.classList.toggle("dark", t === "dark");
}

/** Reactive theme hook. Follows system until the user flips it — after that,
 *  the explicit choice sticks across sessions. */
export function useTheme(): [Theme, (t: Theme) => void] {
  const [theme, setTheme] = useState<Theme>(initial);

  useEffect(() => {
    apply(theme);
  }, [theme]);

  useEffect(() => {
    // Only follow system while the user hasn't made an explicit choice.
    if (localStorage.getItem(KEY)) return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handle = (e: MediaQueryListEvent) => setTheme(e.matches ? "dark" : "light");
    mq.addEventListener("change", handle);
    return () => mq.removeEventListener("change", handle);
  }, []);

  const set = (t: Theme) => {
    localStorage.setItem(KEY, t);
    setTheme(t);
  };
  return [theme, set];
}
