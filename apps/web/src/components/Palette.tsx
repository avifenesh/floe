/** Command palette — RFC: "Scope switching via inline ribbon in the
 *  spine + `/` palette." Opens on `/`, closes on Escape.
 *
 *  Actions cover every scope + sub-view jump: the PR scope, each flow
 *  by name, each sub-tab under the current top-tab, plus a handful of
 *  utilities (sign out, theme toggle). Fuzzy filter is substring-based
 *  against the action label. Arrow keys move, Enter invokes.
 */

import { useEffect, useMemo, useRef, useState } from "react";
import type { Flow } from "@/types/artifact";
import type { FlowSubTab, PrSubTab, TopTab } from "@/views/types";
import { FLOW_SUB_TABS, PR_SUB_TABS } from "@/views/types";

export interface PaletteAction {
  id: string;
  label: string;
  hint?: string;
  run: () => void;
}

interface Props {
  flows: Flow[];
  top: TopTab;
  onTop: (t: TopTab) => void;
  onFlowSub: (s: FlowSubTab) => void;
  onPrSub: (s: PrSubTab) => void;
  extraActions?: PaletteAction[];
}

export function Palette({
  flows,
  top,
  onTop,
  onFlowSub,
  onPrSub,
  extraActions = [],
}: Props) {
  const [open, setOpen] = useState(false);
  const [q, setQ] = useState("");
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement | null>(null);

  // Global keybinding: `/` opens, Escape closes. Ignore when the user
  // is already typing in a text input so the palette doesn't steal
  // a slash character in, say, the PR URL field.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      const t = e.target as HTMLElement | null;
      const isEditing =
        t && (t.tagName === "INPUT" || t.tagName === "TEXTAREA" || t.isContentEditable);
      if (e.key === "/" && !isEditing && !open) {
        e.preventDefault();
        setOpen(true);
        setQ("");
        setCursor(0);
        return;
      }
      if (e.key === "Escape" && open) {
        setOpen(false);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open]);

  useEffect(() => {
    if (open) setTimeout(() => inputRef.current?.focus(), 0);
  }, [open]);

  const actions: PaletteAction[] = useMemo(() => {
    const list: PaletteAction[] = [];
    list.push({
      id: "scope:pr",
      label: "Scope: PR",
      hint: "show the aggregated PR view",
      run: () => onTop({ kind: "pr" }),
    });
    for (const f of flows) {
      list.push({
        id: `scope:flow:${f.id}`,
        label: `Scope: ${f.name}`,
        hint: f.rationale,
        run: () => onTop({ kind: "flow", flowId: f.id }),
      });
    }
    if (top.kind === "flow") {
      for (const t of FLOW_SUB_TABS) {
        list.push({
          id: `view:flow:${t.key}`,
          label: `View: ${t.label}`,
          hint: "flow sub-tab",
          run: () => onFlowSub(t.key),
        });
      }
    } else {
      for (const t of PR_SUB_TABS) {
        list.push({
          id: `view:pr:${t.key}`,
          label: `View: ${t.label}`,
          hint: "PR sub-tab",
          run: () => onPrSub(t.key),
        });
      }
    }
    for (const a of extraActions) list.push(a);
    return list;
  }, [flows, top, onTop, onFlowSub, onPrSub, extraActions]);

  const matches = useMemo(() => {
    const query = q.trim().toLowerCase();
    if (!query) return actions.slice(0, 30);
    return actions
      .filter((a) => a.label.toLowerCase().includes(query) || (a.hint ?? "").toLowerCase().includes(query))
      .slice(0, 30);
  }, [q, actions]);

  useEffect(() => {
    if (cursor >= matches.length) setCursor(0);
  }, [matches.length, cursor]);

  if (!open) return null;

  function invoke(a: PaletteAction) {
    a.run();
    setOpen(false);
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-start justify-center pt-[12vh] bg-black/40 backdrop-blur-sm"
      onClick={() => setOpen(false)}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        className="w-[520px] max-w-[92vw] rounded-lg border border-border/70 bg-background shadow-2xl overflow-hidden"
      >
        <div className="px-3 py-2 border-b border-border/60">
          <input
            ref={inputRef}
            value={q}
            onChange={(e) => {
              setQ(e.target.value);
              setCursor(0);
            }}
            onKeyDown={(e) => {
              if (e.key === "ArrowDown") {
                e.preventDefault();
                setCursor((c) => Math.min(c + 1, matches.length - 1));
              } else if (e.key === "ArrowUp") {
                e.preventDefault();
                setCursor((c) => Math.max(c - 1, 0));
              } else if (e.key === "Enter") {
                e.preventDefault();
                const a = matches[cursor];
                if (a) invoke(a);
              }
            }}
            placeholder="Jump to flow or view…"
            className="w-full bg-transparent text-[13px] font-mono placeholder:text-muted-foreground/60 focus:outline-none"
          />
        </div>
        <ul className="max-h-[50vh] overflow-y-auto py-1">
          {matches.length === 0 ? (
            <li className="px-3 py-2 text-[12px] font-mono text-muted-foreground">
              No matches.
            </li>
          ) : (
            matches.map((a, i) => (
              <li key={a.id}>
                <button
                  onClick={() => invoke(a)}
                  onMouseEnter={() => setCursor(i)}
                  className={
                    "w-full text-left px-3 py-1.5 flex items-baseline gap-3 " +
                    (i === cursor ? "bg-muted/40" : "hover:bg-muted/20")
                  }
                >
                  <span className="text-[12px] font-mono text-foreground truncate">
                    {a.label}
                  </span>
                  {a.hint && (
                    <span className="ml-auto text-[10px] font-mono text-muted-foreground truncate max-w-[50%]">
                      {a.hint}
                    </span>
                  )}
                </button>
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  );
}
