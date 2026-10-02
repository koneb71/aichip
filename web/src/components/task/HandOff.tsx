import { useState } from "react";
import { ArrowRightLeft } from "lucide-react";
import { Agent, api } from "../../lib/api";
import { Button } from "../ui/Button";
import { Select, Textarea } from "../ui/Field";
import { toast } from "../ui/Toast";

/**
 * Give a running card to another agent without losing the work so far.
 *
 * The running agent is stopped and the new one picks up in the same
 * worktree; the note is its brief, so it is required — "why it changed
 * hands" is the one thing the new agent cannot read from the diff.
 */
export function HandOff({
  taskId,
  current,
  agents,
  onDone,
}: {
  taskId: string;
  current: string | null;
  agents: Agent[];
  onDone: () => void;
}) {
  const [open, setOpen] = useState(false);
  const [to, setTo] = useState("");
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const choices = agents.filter((a) => a.id !== current && a.status !== "retired");

  if (!open) {
    return (
      <Button size="xs" variant="ghost" className="mt-1.5" icon={<ArrowRightLeft className="size-3" />} onClick={() => setOpen(true)}>
        Hand off with a note
      </Button>
    );
  }

  const go = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.handOff(taskId, to, note.trim());
      toast("Handing over", { tone: "success", body: "The running agent is stopping; the new one picks up from here." });
      setOpen(false);
      setNote("");
      onDone();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mt-2 space-y-2 rounded-md border border-border bg-panel-2 p-2.5">
      <Select aria-label="Hand to" value={to} onChange={(e) => setTo(e.target.value)}>
        <option value="">Hand to…</option>
        {choices.map((a) => (
          <option key={a.id} value={a.id}>
            {a.name}
          </option>
        ))}
      </Select>
      <Textarea
        aria-label="Note for the next agent"
        className="min-h-[60px]"
        maxLength={2000}
        value={note}
        onChange={(e) => setNote(e.target.value)}
        placeholder="What the next agent should know — what is done, what is not, why it is changing hands."
      />
      {error && <p className="text-[11px] text-danger-fg">{error}</p>}
      <div className="flex gap-2">
        <Button size="sm" variant="primary" loading={busy} disabled={!to || !note.trim()} onClick={() => void go()}>
          Stop and hand off
        </Button>
        <Button size="sm" variant="ghost" onClick={() => setOpen(false)}>
          Cancel
        </Button>
      </div>
    </div>
  );
}
