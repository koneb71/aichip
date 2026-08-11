import { useState } from "react";
import { motion } from "framer-motion";
import { api, type AppDetail } from "../../lib/api";

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
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/25 backdrop-blur-[3px] p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 12, opacity: 0 }}
        animate={{ scale: 1, y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 220, damping: 26 }}
        exit={{ scale: 0.97, y: 8 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-lg rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">Change {app.name}</h3>
        <p className="mt-1 text-xs text-ink-dim">
          {app.runtime === "module"
            ? "An agent rewrites this app's manifest in a worktree."
            : "An agent changes this app's source in a worktree."}
        </p>

        <textarea
          autoFocus
          value={brief}
          onChange={(e) => setBrief(e.target.value)}
          onKeyDown={(e) => {
            // Enter is a newline in a brief that may well be a paragraph.
            if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && !busy) start();
          }}
          placeholder="add a notes field and show it in the list"
          className="mt-3 h-28 w-full resize-none rounded-lg border border-line bg-surface p-3 text-sm outline-none focus:border-accent"
        />

        <div className="mt-3 rounded-lg bg-panel-2 px-3 py-2 text-[11px] leading-relaxed text-ink-dim">
          This lands on its own when the card finishes — there is no review step,
          because the diff <em>is</em> the app. You can undo the most recent change
          from the history below.
          <br />
          New tables and columns apply themselves. Anything that would lose data
          still waits for you.
        </div>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={start}
            disabled={busy}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy ? "Starting…" : "Start"}
          </motion.button>
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
