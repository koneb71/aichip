import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, BakeoffVariant, Tier, tierModel } from "../lib/api";
import { isActive, statusColor, statusLabel } from "../lib/runStatus";
import { annotateDiff } from "../lib/diff";

/**
 * One brief, several attempts, side by side.
 *
 * The activity page can say what each agent costs; nothing could say whether
 * any of them was any good. This is the missing half — the same task run two
 * or three ways in isolated checkouts, real diffs to compare, and one click
 * to adopt the winner and throw the rest away.
 */
export function BakeoffView({
  taskId,
  agents,
  currentTier,
  onKept,
  onClose,
}: {
  taskId: string;
  agents: Agent[];
  currentTier: Tier;
  onKept: () => void;
  onClose: () => void;
}) {
  const [variants, setVariants] = useState<BakeoffVariant[] | null>(null);
  const [selected, setSelected] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(() => {
    api.bakeoff(taskId).then((r) => setVariants(r.variants)).catch(() => {});
  }, [taskId]);

  useEffect(() => {
    load();
    // Attempts finish at different times; polling keeps the comparison honest
    // while any of them is still running.
    const timer = setInterval(load, 4000);
    return () => clearInterval(timer);
  }, [load]);

  const running = variants?.some((v) => isActive(v.status)) ?? false;
  const done = variants?.filter((v) => v.status === "completed") ?? [];

  const keep = async (runId: string) => {
    setBusy(true);
    try {
      await api.keepVariant(runId);
      onKept();
      onClose();
    } finally {
      setBusy(false);
    }
  };

  if (variants && variants.length === 0) {
    return <BakeoffSetup taskId={taskId} agents={agents} currentTier={currentTier} onStarted={load} onClose={onClose} />;
  }

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <button onClick={onClose} className="text-xs text-ink-dim hover:text-ink">
          ← back to stream
        </button>
        {running && (
          <span className="text-[11px] text-ink-dim">
            {done.length} of {variants?.length} finished…
          </span>
        )}
      </div>

      <div className="space-y-3">
        {variants?.map((v) => {
          const isOpen = selected === v.runId;
          const cheapest =
            done.length > 1 &&
            v.costUsd != null &&
            v.costUsd === Math.min(...done.map((d) => d.costUsd ?? Infinity));
          return (
            <motion.div
              key={v.runId}
              layout
              className="card-shadow rounded-xl border border-line bg-panel"
            >
              <div className="flex flex-wrap items-center gap-2 p-3">
                <span
                  className="h-2 w-2 shrink-0 rounded-full"
                  style={{ background: statusColor(v.status) }}
                />
                <span className="text-sm font-semibold">{v.label}</span>
                {cheapest && (
                  <span className="rounded-full bg-tier-easy-soft px-2 py-0.5 text-[11px] text-tier-easy">
                    cheapest
                  </span>
                )}
                <span className="text-[11px] text-ink-dim">
                  {[
                    v.agentName,
                    v.model,
                    v.costUsd != null ? `$${v.costUsd.toFixed(3)}` : null,
                    v.seconds != null ? `${Math.round(v.seconds / 60)}m` : null,
                    v.status === "completed" ? `${v.linesChanged} lines` : statusLabel(v.status),
                  ]
                    .filter(Boolean)
                    .join(" · ")}
                </span>
                <div className="ml-auto flex gap-2">
                  {v.diff && (
                    <button
                      onClick={() => setSelected(isOpen ? null : v.runId)}
                      className="rounded-lg border border-line px-2.5 py-1 text-xs hover:bg-panel-2"
                    >
                      {isOpen ? "Hide diff" : "See diff"}
                    </button>
                  )}
                  <button
                    onClick={() => keep(v.runId)}
                    disabled={busy || v.status !== "completed"}
                    title={
                      v.status === "completed"
                        ? "Adopt this attempt and discard the others"
                        : "Only a finished attempt can be kept"
                    }
                    className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white disabled:opacity-40"
                  >
                    Keep this
                  </button>
                </div>
              </div>

              {v.error && (
                <div className="mx-3 mb-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
                  {v.error}
                </div>
              )}

              <AnimatePresence>
                {isOpen && (
                  <motion.div
                    initial={{ opacity: 0 }}
                    animate={{ opacity: 1 }}
                    exit={{ opacity: 0 }}
                    className="border-t border-line"
                  >
                    <pre className="max-h-80 overflow-auto bg-panel-2 p-3 font-mono text-xs leading-relaxed">
                      {annotateDiff(v.diff).map((line, i) => (
                        <div
                          key={i}
                          className={
                            line.kind === "add"
                              ? "text-tier-easy"
                              : line.kind === "del"
                                ? "text-red-400"
                                : line.kind === "hunk"
                                  ? "text-tier-medium"
                                  : "text-ink-dim"
                          }
                        >
                          {line.text || " "}
                        </div>
                      ))}
                    </pre>
                  </motion.div>
                )}
              </AnimatePresence>
            </motion.div>
          );
        })}
      </div>

      <p className="mt-3 text-[11px] text-ink-dim">
        Keeping one adopts its branch as this card's work and deletes the other
        checkouts. What they cost stays on the record.
      </p>
    </div>
  );
}

const TIERS: Tier[] = ["easy", "medium", "complex"];

/** Choosing what to compare. Two attempts minimum — one isn't a comparison. */
function BakeoffSetup({
  taskId,
  agents,
  currentTier,
  onStarted,
  onClose,
}: {
  taskId: string;
  agents: Agent[];
  currentTier: Tier;
  onStarted: () => void;
  onClose: () => void;
}) {
  const [mode, setMode] = useState<"tiers" | "agents">("tiers");
  const [tiers, setTiers] = useState<Tier[]>(
    TIERS.filter((t) => t !== currentTier).slice(0, 1).concat(currentTier),
  );
  const [picked, setPicked] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const variants =
        mode === "tiers"
          ? tiers.map((t) => ({ label: tierModel[t], tier: t }))
          : picked.map((id) => ({
              label: agents.find((a) => a.id === id)?.name ?? "agent",
              agent_id: id,
            }));
      await api.startBakeoff(taskId, variants);
      onStarted();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const count = mode === "tiers" ? tiers.length : picked.length;

  return (
    <div>
      <button onClick={onClose} className="mb-3 text-xs text-ink-dim hover:text-ink">
        ← back to stream
      </button>
      <div className="text-sm font-semibold">Run this task more than one way</div>
      <p className="mt-1 text-xs text-ink-dim">
        Each attempt works in its own checkout and never sees the others. When
        they finish you compare the diffs and keep one.
      </p>

      <div className="mt-3 flex gap-2">
        {(["tiers", "agents"] as const).map((m) => (
          <button
            key={m}
            onClick={() => setMode(m)}
            className={`rounded-lg border px-3 py-1.5 text-xs ${
              mode === m ? "border-accent bg-accent/5 text-accent" : "border-line"
            }`}
          >
            {m === "tiers" ? "Compare models" : "Compare agents"}
          </button>
        ))}
      </div>

      <div className="mt-3 space-y-1.5">
        {mode === "tiers"
          ? TIERS.map((t) => (
              <label
                key={t}
                className="flex cursor-pointer items-center gap-2 rounded-lg border border-line p-2.5 text-sm hover:bg-panel-2"
              >
                <input
                  type="checkbox"
                  checked={tiers.includes(t)}
                  onChange={(e) =>
                    setTiers((prev) =>
                      e.target.checked ? [...prev, t] : prev.filter((x) => x !== t),
                    )
                  }
                  className="accent-[var(--color-accent)]"
                />
                <span className="capitalize">{t}</span>
                <span className="text-xs text-ink-dim">{tierModel[t]}</span>
              </label>
            ))
          : agents.map((a) => (
              <label
                key={a.id}
                className="flex cursor-pointer items-center gap-2 rounded-lg border border-line p-2.5 text-sm hover:bg-panel-2"
              >
                <input
                  type="checkbox"
                  checked={picked.includes(a.id)}
                  onChange={(e) =>
                    setPicked((prev) =>
                      e.target.checked ? [...prev, a.id] : prev.filter((x) => x !== a.id),
                    )
                  }
                  className="accent-[var(--color-accent)]"
                />
                <span className="h-2 w-2 rounded-full" style={{ background: a.color }} />
                {a.name}
              </label>
            ))}
      </div>

      {error && (
        <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      <div className="mt-4 flex items-center gap-3">
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={start}
          disabled={busy || count < 2}
          className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white disabled:opacity-50"
        >
          {busy ? "Starting…" : `Run ${count || 0} attempts`}
        </motion.button>
        <span className="text-[11px] text-ink-dim">
          {count < 2
            ? "Pick at least two."
            : `${count} runs against your rate limit, at once.`}
        </span>
      </div>
    </div>
  );
}
