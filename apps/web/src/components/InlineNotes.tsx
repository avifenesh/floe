import { useState } from "react";
import type { InlineNote, InlineNoteAnchor } from "@/types/artifact";
import { addInlineNote, deleteInlineNote, exportInlineNotes } from "@/api";

/** Inline comment thread anchored to one reviewable object.
 *
 * Mirrors GitHub's line-comment affordance, but generalised: `anchor`
 * may point at a flow, entity, intent claim, hunk, or file-line. Renders
 * any existing notes for this anchor plus a collapsed "add note"
 * textarea. The "copy for agent" button calls the export endpoint and
 * copies just this note's entry to the clipboard.
 *
 * Parent is responsible for persisting the artifact's `inline_notes`
 * back into state after `onChange`.
 */
export function InlineNotes({
  jobId,
  anchor,
  notes,
  onChange,
  label = "note",
}: {
  jobId: string;
  anchor: InlineNoteAnchor;
  notes: InlineNote[];
  onChange: (next: InlineNote[]) => void;
  label?: string;
}) {
  const mine = notes.filter((n) => sameAnchor(n.anchor, anchor));
  const [open, setOpen] = useState(false);
  const [text, setText] = useState("");
  const [busy, setBusy] = useState(false);

  async function submit() {
    const trimmed = text.trim();
    if (!trimmed) return;
    setBusy(true);
    try {
      const saved = await addInlineNote(jobId, anchor, trimmed);
      onChange([...notes, saved]);
      setText("");
      setOpen(false);
    } catch (e) {
      console.error("add note failed", e);
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
      console.error("delete note failed", e);
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
      if (match) {
        await navigator.clipboard.writeText(JSON.stringify(match, null, 2));
      }
    } catch (e) {
      console.error("copy export failed", e);
    }
  }

  return (
    <div className="mt-1 space-y-1">
      {mine.length > 0 && (
        <ul className="space-y-1">
          {mine.map((n) => (
            <li
              key={n.id}
              className="rounded border border-border/60 bg-background px-2 py-1.5 text-[11px] space-y-1"
            >
              <div className="flex items-baseline gap-2 text-muted-foreground">
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
          ))}
        </ul>
      )}
      {open ? (
        <div className="space-y-1">
          <textarea
            value={text}
            onChange={(e) => setText(e.target.value)}
            placeholder={`Leave a ${label}…`}
            rows={3}
            className="w-full rounded border border-border/60 bg-background px-2 py-1 text-[12px] font-mono"
            disabled={busy}
          />
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={submit}
              disabled={busy || !text.trim()}
              className="rounded border border-border/60 px-2 py-0.5 text-[11px] hover:bg-muted disabled:opacity-50"
            >
              save
            </button>
            <button
              type="button"
              onClick={() => {
                setOpen(false);
                setText("");
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
          {mine.length > 0 ? "+ add another" : `💬 add ${label}`}
        </button>
      )}
    </div>
  );
}

function sameAnchor(a: InlineNoteAnchor, b: InlineNoteAnchor): boolean {
  if (a.kind !== b.kind) return false;
  switch (a.kind) {
    case "hunk":
      return a.hunk_id === (b as typeof a).hunk_id;
    case "flow":
      return a.flow_id === (b as typeof a).flow_id;
    case "entity":
      return a.entity_name === (b as typeof a).entity_name;
    case "intent-claim":
      return a.claim_index === (b as typeof a).claim_index;
    case "file-line": {
      const bb = b as typeof a;
      return a.file === bb.file && a.line_side === bb.line_side && a.line === bb.line;
    }
  }
}
