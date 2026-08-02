import { useState } from "react";
import { motion } from "framer-motion";
import { api } from "../../lib/api";

/**
 * A starting manifest, so the first thing someone sees is a working app rather
 * than an empty box and the word YAML.
 *
 * Chosen to exercise most of the format in as few lines as possible: two field
 * types, a computed column, a default, a list and a chart. Editing it is how
 * most people will learn the shape.
 */
const STARTER = `name: Expenses
icon: "▤"
summary: Track spending by category
runtime: module

models:
  expense:
    fields:
      description: { type: text, required: true }
      amount:      { type: decimal }
      qty:         { type: int, default: 1 }
      total:       { type: decimal, compute: "amount * qty" }
      spent_on:    { type: date, default: "today()" }
      category:    { type: text }
    indexes: [spent_on]

views:
  list:
    columns: [spent_on, description, category, total]
    sort: "-spent_on"
  chart:
    shape: bar
    group_by: category
    measure: "sum(total)"

menu:
  - { label: Expenses, view: list }
  - { label: By category, view: chart }
`;

export function NewAppModal({
  workspaceId,
  onClose,
  onInstalled,
}: {
  workspaceId: string;
  onClose: () => void;
  onInstalled: () => void;
}) {
  const [manifest, setManifest] = useState(STARTER);
  const [brief, setBrief] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [writing, setWriting] = useState(false);

  /**
   * Ask an agent for a manifest, and put it in the box rather than installing
   * it. Reading the thing before it becomes real is the whole reason an app is
   * a declaration instead of code.
   */
  const generate = async () => {
    if (!brief.trim()) {
      setError("Say what the app is for first.");
      return;
    }
    setWriting(true);
    setError(null);
    try {
      const r = await api.generateApp(brief.trim());
      setManifest(r.manifest);
      // A manifest that does not parse still lands in the box, with the
      // parser's complaint above it: the fix is usually one line, and
      // throwing the draft away to regenerate costs another call.
      setError(r.error);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setWriting(false);
    }
  };

  const install = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.installApp(workspaceId, manifest, brief);
      onInstalled();
    } catch (e) {
      // The parser names the key — "models.expense.fields.qty: unknown field
      // type" — so it is shown as it came rather than summarised.
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
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.97, y: 8 }}
        transition={{ type: "spring", stiffness: 420, damping: 32 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex max-h-[88vh] w-full max-w-2xl flex-col rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">New app</h3>
        <p className="mt-1 text-xs text-ink-dim">
          An app is a manifest: models become real tables, views become screens. Nothing in
          here executes.
        </p>

        <div className="mt-4">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            What is it for
          </span>
          <div className="flex gap-2">
            <input
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && !writing) generate();
              }}
              placeholder="track my spending by category"
              className="flex-1 rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent"
            />
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={generate}
              disabled={writing || busy}
              className="shrink-0 rounded-lg border border-line px-3 py-1.5 text-xs hover:bg-line/40 disabled:opacity-50"
            >
              {writing ? "Writing…" : "Write it for me"}
            </motion.button>
          </div>
        </div>

        <label className="mt-3 flex min-h-0 flex-1 flex-col">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Manifest
          </span>
          <textarea
            value={manifest}
            onChange={(e) => setManifest(e.target.value)}
            spellCheck={false}
            className="min-h-[18rem] flex-1 resize-none rounded-lg border border-line bg-surface p-3 font-mono text-xs outline-none focus:border-accent"
          />
        </label>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 font-mono text-[11px] text-danger">
            {error}
          </div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={install}
            disabled={busy}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy ? "Installing…" : "Install"}
          </motion.button>
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
