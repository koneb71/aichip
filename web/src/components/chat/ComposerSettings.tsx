import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Effort, Tier } from "../../lib/api";
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
  effort,
  onEffort,
  disabled,
}: {
  engine: string | null;
  onEngine: (next: string | null) => void;
  tier: Tier;
  onTier: (next: Tier) => void;
  effort: Effort | null;
  onEffort: (next: Effort | null) => void;
  disabled?: boolean;
}) {
  const [open, setOpen] = useState(false);
  const engines = useEngines();
  const tierModel = useTierModel();
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
