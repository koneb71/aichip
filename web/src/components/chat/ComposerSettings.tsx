import { useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Effort, LocalModel, Tier, api } from "../../lib/api";
import { useTierModel } from "../../lib/models";
import { EnginePicker, useEngines } from "../../lib/engines";
import { TierPicker } from "../TierPicker";
import { EffortPicker, EFFORTS } from "../EffortPicker";

/**
 * What this conversation runs as, folded into one line.
 *
 * Three dropdowns sat open under the composer and wrapped onto a second row in
 * a panel this narrow — a permanent cost for a choice most people set once and
 * never touch. Collapsed to a summary that reads as a sentence and opens the
 * pickers when there is something to change.
 *
 * The summary is the point: it has to say what will happen without being
 * clicked, or this is just a hidden control.
 */
export function ComposerSettings({
  engine,
  onEngine,
  tier,
  onTier,
  modelId,
  onModelId,
  effort,
  onEffort,
  disabled,
}: {
  engine: string | null;
  onEngine: (next: string | null) => void;
  tier: Tier;
  onTier: (next: Tier) => void;
  /** One conversation on one model. Empty resolves from the tier. */
  modelId: string;
  onModelId: (next: string) => void;
  effort: Effort | null;
  onEffort: (next: Effort | null) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const engines = useEngines();
  const tierModel = useTierModel();
  // Suggestions only: any id the engine accepts is legitimate here, so this
  // is a datalist rather than a select.
  const [localModels, setLocalModels] = useState<LocalModel[]>([]);
  useEffect(() => {
    api.localModels().then((r) => setLocalModels(r.models)).catch(() => {});
  }, []);
  const manyEngines = !!engines && engines.length > 1;

  const summary = [
    // Only worth naming when there is more than one and this isn't the default
    // — "Claude Code" on a machine that has only Claude Code says nothing.
    manyEngines ? engines?.find((e) => e.id === engine)?.label : null,
    tierModel(tier, engine ?? undefined),
    effort
      ? `${EFFORTS.find((e) => e.id === effort)?.label.toLowerCase()} thinking`
      : null,
  ].filter(Boolean);

  return (
    <div className="relative">
      <button
        onClick={() => setOpen((o) => !o)}
        disabled={disabled}
        className="flex items-center gap-1 rounded-md px-1 py-0.5 text-[11px] text-ink-dim hover:bg-line/40 hover:text-ink disabled:opacity-50"
      >
        {summary.join(" · ")}
        <span className={`transition-transform ${open ? "rotate-180" : ""}`}>⌄</span>
      </button>

      <AnimatePresence>
        {open && (
          <>
            {/* Click-away layer, below the popover and above the panel. */}
            <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
            <motion.div
              initial={{ opacity: 0, y: 4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: 4 }}
              className="card-shadow absolute bottom-full left-0 z-20 mb-2 w-64 space-y-2 rounded-xl border border-line bg-panel p-3"
            >
              {manyEngines && (
                <Row label="Run on">
                  <EnginePicker
                    value={engine}
                    onChange={onEngine}
                    inheritLabel="Default"
                  />
                </Row>
              )}
              <Row label="Model">
                <TierPicker value={tier} onChange={onTier} engine={engine ?? undefined} />
                {/* Below the tier, not instead of it. The tier is the usual
                    answer and stays the default; this is the escape hatch for
                    "just this conversation, this model".

                    The models are listed as buttons rather than hidden in a
                    datalist. A datalist shows nothing until you guess what to
                    type, which meant somebody with LM Studio running still had
                    no way to tell aichip could see it — the discovery worked
                    and the person could not find it, which is the same as it
                    not working. */}
                <div className="mt-2">
                  <span className="mb-1 block text-[10px] font-semibold uppercase tracking-wide text-ink-dim">
                    Or one specific model
                  </span>
                  <input
                    value={modelId}
                    onChange={(e) => onModelId(e.target.value)}
                    spellCheck={false}
                    placeholder="leave empty to use the tier"
                    className="w-full rounded-lg border border-line bg-panel px-2 py-1.5 font-mono text-[11px]"
                  />
                  {localModels.length > 0 && (
                    <div className="mt-1.5">
                      <span className="text-[10px] text-ink-dim">
                        On this machine — click to use:
                      </span>
                      <div className="mt-1 flex flex-wrap gap-1">
                        {localModels.map((m) => (
                          <button
                            key={m.id}
                            type="button"
                            onClick={() => onModelId(modelId === m.id ? "" : m.id)}
                            title={m.id}
                            className={`ring-focus max-w-full truncate rounded-lg border px-1.5 py-0.5 font-mono text-[10px] ${
                              modelId === m.id
                                ? "border-accent bg-accent/10 text-accent"
                                : "border-line text-ink-dim hover:border-accent/50"
                            }`}
                          >
                            {m.name}
                          </button>
                        ))}
                      </div>
                      {/* Said once, here, because "why is Ollama not in Run on"
                          is the question this layout provokes. */}
                      <p className="mt-1 text-[10px] leading-relaxed text-ink-dim">
                        Served by Ollama or LM Studio and run through OpenCode — pick that engine
                        above.
                      </p>
                    </div>
                  )}
                  {modelId.trim() !== "" && (
                    <span className="mt-1 block text-[10px] text-amber-700">
                      This chat runs on {modelId.trim()}, ignoring the tier.
                    </span>
                  )}
                </div>
              </Row>
              <Row label="Thinking">
                <EffortPicker value={effort} onChange={onEffort} />
              </Row>
              <p className="text-[11px] text-ink-dim">
                Sticks to this conversation, not just the next message.
              </p>
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
        {label}
      </div>
      {children}
    </div>
  );
}
