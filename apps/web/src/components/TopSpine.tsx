import { cn } from "@/lib/cn";
import { useTheme } from "@/lib/theme";

import { VIEW_KEYS, VIEW_LABELS, type ViewKey } from "@/views/types";
import { Kbd } from "./Kbd";

interface Props {
  view: ViewKey;
  onView: (v: ViewKey) => void;
  prLabel: string | null;
}

/**
 * Single 40px hairline spine. Three zones:
 *   left  · PR identity (mono)
 *   center · seven view labels — muted by default, active gets weight + color
 *           + 2px underline. No container pills.
 *   right · quiet `/` palette hint
 *
 * No border-bottom — chrome dissolves into the view.
 */
export function TopSpine({ view, onView, prLabel }: Props) {
  const [theme, setTheme] = useTheme();
  return (
    <header className="h-10 flex items-center">
      <div className="w-full max-w-6xl mx-auto px-6 grid grid-cols-[1fr,auto,1fr] items-center">
        <div className="justify-self-start text-[12px] font-mono text-muted-foreground truncate">
          {prLabel ?? "No PR loaded"}
        </div>

        <nav className="flex items-center gap-5">
          {VIEW_KEYS.map((k) => (
            <button
              key={k}
              onClick={() => onView(k)}
              className={cn(
                "text-[13px] tracking-tight transition-colors relative py-1",
                k === view
                  ? "text-foreground font-semibold"
                  : "text-muted-foreground font-medium hover:text-foreground",
              )}
            >
              {VIEW_LABELS[k]}
              {k === view && (
                <span
                  aria-hidden
                  className="absolute left-0 right-0 -bottom-[1px] h-[2px] bg-foreground rounded-full"
                />
              )}
            </button>
          ))}
        </nav>

        <div className="justify-self-end flex items-center gap-3">
          <button
            onClick={() => setTheme(theme === "dark" ? "light" : "dark")}
            className="text-[11px] font-mono text-muted-foreground hover:text-foreground transition-colors"
            aria-label={`Switch to ${theme === "dark" ? "light" : "dark"} mode`}
          >
            {theme === "dark" ? "◐ Dark" : "○ Light"}
          </button>
          <div className="flex items-center gap-1.5">
            <span className="text-[11px] font-mono text-muted-foreground">Palette</span>
            <Kbd>/</Kbd>
          </div>
        </div>
      </div>
    </header>
  );
}
