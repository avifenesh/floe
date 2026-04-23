import { useState } from "react";
import type { InlineNote } from "@/types/artifact";
import { addInlineNote, deleteInlineNote, exportInlineNotes } from "@/api";

/** File-line note panel. Renders existing file-line notes for the
 *  selected file + a compact form to add a new one (line + side +
 *  text). Proper gutter UI on DiffView is a future refinement; this
 *  lands the data path. */
export function FileLineNotes({
  jobId,
  file,
  notes,
  onChange,
}: {
  jobId: string;
  file: string;
  notes: InlineNote[];
  onChange: (next: InlineNote[]) => void;
}) {
  const fileNotes = notes.filter(
    (n) => n.anchor.kind === "file-line" && n.anchor.file === file,
  );
  const [line, setLine] = useState<string>("");
  const [side, setSide] = useState<"base" | "head">("head");
  const [text, setText] = useState<string>("");
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);

  async function submit() {
    const lineNum = Number(line);
    if (!Number.isFinite(lineNum) || lineNum < 1) return;
    if (!text.trim()) return;
    setBusy(true);
    try {
      const saved = await addInlineNote(
        jobId,
        { kind: "file-line", file, line_side: side, line: lineNum },
        text.trim(),
      );
      onChange([...notes, saved]);
      setText("");
      setLine("");
      setOpen(false);
    } catch (e) {
      console.error("add file-line note", e);
    } finally {
      setBusy(false);
    }
  }

  async function remove(id: string) {
    setBusy(true);
    try {
      await deleteInlineNote(jobId, id);
      onChange(notes.filter((n) => n.id !== id));
    } catch (e) {
      console.error("delete file-line note", e);
    } finally {
      setBusy(false);
    }
  }

  async function copyForAgent(id: string) {
    try {
      const bundle = (await exportInlineNotes(jobId)) as {
        notes: Array<{ id: string } & Record<string, unknown>>;
      };
      const match = bundle.notes.find((n) => n.id === id);
      if (match) await navigator.clipboard.writeText(JSON.stringify(match, null, 2));
    } catch (e) {
      console.error("copy export", e);
    }
  }

  return (
    <section className="rounded border border-border/60 bg-muted/10 px-3 py-2 space-y-2">
      <h3 className="text-[11px] font-medium text-muted-foreground uppercase tracking-wide">
        Line notes · {file}
      </h3>
      {fileNotes.length > 0 ? (
        <ul className="space-y-1">
          {fileNotes.map((n) => {
            if (n.anchor.kind !== "file-line") return null;
            return (
              <li
                key={n.id}
                className="rounded border border-border/60 bg-background px-2 py-1.5 text-[11px] space-y-1"
              >
                <div className="flex items-baseline gap-2 text-muted-foreground">
                  <span className="font-mono text-[10px]">
                    {n.anchor.line_side}:{n.anchor.line}
                  </span>
                  <span className="font-mono text-[10px]">{n.author}</span>
                  <span className="font-mono text-[10px]">
                    {new Date(n.created_at).toLocaleString()}
                  </span>
                  <button
                    type="button"
                    onClick={() => copyForAgent(n.id)}
                    className="ml-auto text-[10px] hover:text-foreground"
                    disabled={busy}
                  >
                    copy for agent
                  </button>
                  <button
                    type="button"
                    onClick={() => remove(n.id)}
                    className="text-[10px] hover:text-rose-400"
                    disabled={busy}
                  >
                    delete
                  </button>
                </div>
                <p className="text-foreground whitespace-pre-wrap">{n.text}</p>
              </li>
            );
          })}
        </ul>
      ) : (
        <p className="text-[11px] text-muted-foreground italic">
          No line notes yet.
        </p>
      )}
      {open ? (
        <div className="space-y-1">
          <div className="flex items-center gap-2">
            <label className="text-[11px] text-muted-foreground">side</label>
            <select
              value={side}
              onChange={(e) => setSide(e.target.value as "base" | "head")}
              className="rounded border border-border/60 bg-background text-[11px] px-1 py-0.5"
              disabled={busy}
            >
              <option value="head">head</option>
              <option value="base">base</option>
            </select>
            <label className="text-[11px] text-muted-foreground">line</label>
            <input
              type="number"
              min={1}
              value={line}
              onChange={(e) => setLine(e.target.value)}
              className="w-20 rounded border border-border/60 bg-background text-[11px] px-1 py-0.5"
              disabled={busy}
            />
          </div>
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder="Note…"
            rows={2}
            className="w-full rounded border border-border/60 bg-background px-2 py-1 text-[12px] font-mono"
            disabled={busy}
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={submit}
              disabled={busy || !text.trim() || !line}
              className="rounded border border-border/60 px-2 py-0.5 text-[11px] hover:bg-muted disabled:opacity-50"
            >
              save
            </button>
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                setText("");
                setLine("");
              }}
              disabled={busy}
              className="text-[11px] text-muted-foreground hover:text-foreground"
            >
              cancel
            </button>
          </div>
        </div>
      ) : (
        <button
          type="button"
          onClick={() => setOpen(true)}
          className="text-[11px] text-muted-foreground hover:text-foreground"
        >
          💬 add line note
        </button>
      )}
    </section>
  );
}
