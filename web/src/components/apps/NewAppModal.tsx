import { useState } from "react";
import { motion } from "framer-motion";
import { api, type AppRuntime } from "../../lib/api";
import { runtimeBlurb, starterManifest } from "../../lib/apps";

const RUNTIMES: { id: AppRuntime; label: string }[] = [
  { id: "module", label: "Module" },
  { id: "node", label: "Node" },
  { id: "static", label: "Static" },
];

export function NewAppModal({
  workspaceId,
  onClose,
  onInstalled,
}: {
  workspaceId: string;
  onClose: () => void;
  onInstalled: () => void;
}) {
  const [runtime, setRuntime] = useState<AppRuntime>("module");
  const [manifest, setManifest] = useState(() => starterManifest("module"));
  const [brief, setBrief] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [writing, setWriting] = useState(false);
  const [polish, setPolish] = useState(false);

  /**
   * Switching runtime replaces the manifest, because the two starters declare
   * different things and a container app's `views:` block would be refused.
   * An edited manifest is kept rather than silently thrown away — losing typing
   * to a radio button is worse than an inconsistent example.
   */
  const chooseRuntime = (next: AppRuntime) => {
    setRuntime(next);
    setManifest((current) =>
      current === starterManifest(runtime) ? starterManifest(next) : current,
    );
  };

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
      const r = await api.generateApp(brief.trim(), runtime);
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
      const app = await api.installApp(workspaceId, manifest, brief);
      // The scaffolded screens already work; this sends an agent to make them
      // *good*. Fired after install so a failed run costs nothing but the run —
      // the app is installed either way, and the card lands like any other
      // change. Best-effort: an error here is the card's problem, not the
      // install's.
      if (polish && app.runtime !== "module" && brief.trim()) {
        await api.changeApp(app.id, brief.trim()).catch(() => {});
      }
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
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/25 backdrop-blur-[3px] p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 12, opacity: 0 }}
        animate={{ scale: 1, y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 220, damping: 26 }}
        exit={{ scale: 0.97, y: 8 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex max-h-[88vh] w-full max-w-2xl flex-col rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">New app</h3>
        <p className="mt-1 text-xs text-ink-dim">
          Every app declares models, which become real tables. What draws it is the runtime.
        </p>

        <div className="mt-4 flex items-center gap-1">
          {RUNTIMES.map((r) => (
            <button
              key={r.id}
              onClick={() => chooseRuntime(r.id)}
              className={
                "rounded-lg border px-2.5 py-1 text-xs " +
                (runtime === r.id
                  ? "border-accent bg-accent/10 font-medium text-ink"
                  : "border-line text-ink-dim hover:bg-line/40")
              }
            >
              {r.label}
            </button>
          ))}
          {/* Said at the moment of choosing rather than on the failed build:
              a container app on a machine without Docker installs fine and
              then never runs, which looks like a bug in the app. */}
          <span className="ml-2 min-w-0 flex-1 truncate text-[11px] text-ink-dim">
            {runtimeBlurb(runtime)}
          </span>
        </div>

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

        {runtime !== "module" && (
          <label className="mt-3 flex items-center gap-2 text-xs text-ink-dim">
            <input
              type="checkbox"
              checked={polish}
              onChange={(e) => setPolish(e.target.checked)}
            />
            Have an agent polish the screens after install (costs a run)
          </label>
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
