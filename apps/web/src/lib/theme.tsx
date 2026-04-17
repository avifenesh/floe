import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useState,
  type ReactNode,
} from "react";

export type Theme = "light" | "dark";

const KEY = "adr.theme";

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

type Ctx = [Theme, (t: Theme) => void];

const ThemeContext = createContext<Ctx>(["light", () => {}]);

/**
 * Single theme state shared across the app. Any consumer of `useTheme` sees
 * the same value, so toggling in the spine immediately re-tokenises the
 * Source view (and anywhere else that reads the theme).
 *
 * We follow `prefers-color-scheme` until the user makes an explicit choice
 * via the toggle; after that, the stored preference wins.
 */
export function ThemeProvider({ children }: { children: ReactNode }) {
  const [theme, setTheme] = useState<Theme>(initial);

  useEffect(() => {
    apply(theme);
  }, [theme]);

  useEffect(() => {
    if (localStorage.getItem(KEY)) return;
    const mq = window.matchMedia("(prefers-color-scheme: dark)");
    const handle = (e: MediaQueryListEvent) => setTheme(e.matches ? "dark" : "light");
    mq.addEventListener("change", handle);
    return () => mq.removeEventListener("change", handle);
  }, []);

  const set = useCallback((t: Theme) => {
    localStorage.setItem(KEY, t);
    setTheme(t);
  }, []);

  return (
    <ThemeContext.Provider value={[theme, set]}>{children}</ThemeContext.Provider>
  );
}

export function useTheme(): Ctx {
  return useContext(ThemeContext);
}
