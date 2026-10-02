import { useState } from "react";
import { api, type AppDetail } from "../../lib/api";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Textarea } from "../ui/Field";

/**
 * Hand the app to an agent.
 *
 * The two sentences under the box are the whole point of this dialog. A card
 * against your own code stops in review; this one lands by itself, and the only
 * honest way to offer that is to say so *before* the run starts rather than
 * afterwards. The second sentence is the compensation: the undo is real.
 */
export function ChangeAppModal({
  app,
  onClose,
  onStarted,
}: {
  app: AppDetail;
  onClose: () => void;
  onStarted: () => void;
}) {
  const [brief, setBrief] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    if (!brief.trim()) {
      setError("Say what should change.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.changeApp(app.id, brief.trim());
      onStarted();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      title={`Change ${app.name}`}
      description={
        app.runtime === "module"
          ? "An agent rewrites this app's manifest in a worktree."
          : "An agent changes this app's source in a worktree."
      }
      width={512}
      footer={
        <>
          <Button variant="ghost" size="sm" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" size="sm" onClick={start} disabled={busy}>
            {busy ? "Starting…" : "Start"}
          </Button>
        </>
      }
    >
      <Textarea
        autoFocus
        value={brief}
        onChange={(e) => setBrief(e.target.value)}
        onKeyDown={(e) => {
          // Enter is a newline in a brief that may well be a paragraph.
          if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !busy) start();
        }}
        placeholder="add a notes field and show it in the list"
        className="h-28 resize-none p-3! text-sm!"
      />

      <div className="mt-3 rounded-lg bg-panel-2 px-3 py-2 text-[11px] leading-relaxed text-fg-muted">
        This lands on its own when the card finishes — there is no review step,
        because the diff <em>is</em> the app. You can undo the most recent change
        from the history below.
        <br />
        New tables and columns apply themselves. Anything that would lose data
        still waits for you.
      </div>

      {error && (
        <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">{error}</div>
      )}
    </Dialog>
  );
}
