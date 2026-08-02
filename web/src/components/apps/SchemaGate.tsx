import { useState } from "react";
import { motion } from "framer-motion";
import { api, type SchemaPlan } from "../../lib/api";

/**
 * A change to an app's tables that destroys something, waiting to be read.
 *
 * The literal SQL, in full, with a sentence per statement saying what it costs.
 * Shaped like `RecipeGate` and for the same reason: what a person approves has
 * to be the thing that runs, and showing a summary instead would make the
 * approval about a paraphrase.
 *
 * Additive changes never reach here — they have already applied. A dialog that
 * always gets the same answer trains people not to read it.
 */
export function SchemaGate({
  appId,
  plan,
  onDone,
}: {
  appId: string;
  plan: SchemaPlan;
  onDone: () => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const act = async (apply: boolean) => {
    setBusy(true);
    setError(null);
    try {
      if (apply) await api.applyAppSchema(appId, plan.id);
      else await api.discardAppSchema(appId, plan.id);
      onDone();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  const destructive = plan.statements.filter((s) => s.destructive);

  return (
    <div className="mb-4 rounded-xl border border-amber-300 bg-amber-50 p-4">
      <div className="text-sm font-semibold text-amber-900">
        {destructive.length === 1
          ? "This app wants to change its tables in a way that loses data."
          : `This app wants to make ${destructive.length} changes that lose data.`}
      </div>
      <p className="mt-1 text-xs text-amber-900/80">
        Nothing has run. The manifest is saved, but its tables are still as they were.
      </p>

      <div className="mt-3 flex flex-col gap-2">
        {plan.statements.map((s, i) => (
          <div
            key={i}
            className={
              "rounded-lg border p-2 " +
              (s.destructive ? "border-amber-400 bg-white" : "border-line bg-white/60")
            }
          >
            <div className="text-xs text-ink">
              {s.destructive && <span className="mr-1 font-semibold text-danger">Destroys:</span>}
              {s.why}
            </div>
            <pre className="mt-1 overflow-x-auto whitespace-pre-wrap break-all font-mono text-[11px] text-ink-dim">
              {s.sql}
            </pre>
          </div>
        ))}
      </div>

      {error && <div className="mt-3 text-xs text-danger">{error}</div>}

      <div className="mt-4 flex items-center gap-2">
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={() => act(true)}
          disabled={busy}
          className="rounded-lg bg-amber-900 px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          Run these
        </motion.button>
        <button
          onClick={() => act(false)}
          disabled={busy}
          className="rounded-lg border border-amber-400 px-3 py-1.5 text-xs text-amber-900"
        >
          Leave the tables alone
        </button>
      </div>
    </div>
  );
}
