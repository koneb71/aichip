import { useEffect, useState } from "react";
import { Effort, LocalModel, Tier, api } from "../../lib/api";
import { useTierModel } from "../../lib/models";
import { EnginePicker, ToolsNote, useEngines } from "../../lib/engines";
import { TierPicker } from "../TierPicker";
import { EffortPicker, EFFORTS } from "../EffortPicker";
import { Popover } from "../ui/Overlay";
import { Input } from "../ui/Field";

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
  usesTools = true,
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
  /** False for a chat with no project: it is never handed Eren's tools, so
   *  an engine without them is no reason to warn. */
  usesTools?: boolean;
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
    <Popover
      open={open}
      onOpenChange={setOpen}
      className="w-64 space-y-2"
      trigger={
        <button
          disabled={disabled}
          className="ring-focus flex items-center gap-1 rounded-md px-1 py-0.5 text-[11px] text-fg-muted hover:bg-panel-2 hover:text-fg disabled:opacity-50"
        >
          {summary.join(" · ")}
          <span className={`transition-transform ${open ? "rotate-180" : ""}`}>⌄</span>
        </button>
      }
    >
      {manyEngines && (
        <Row label="Run on">
          <EnginePicker
            value={engine}
            onChange={onEngine}
            inheritLabel="Default"
          />
          {usesTools && <ToolsNote engine={engine} what="the assistant" inheritsDefault />}
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
            no way to tell Eren could see it — the discovery worked
            and the person could not find it, which is the same as it
            not working. */}
        <div className="mt-2">
          <span className="mb-1 block text-[10px] font-semibold uppercase tracking-wide text-fg-muted">
            Or one specific model
          </span>
          <Input
            value={modelId}
            onChange={(e) => onModelId(e.target.value)}
            spellCheck={false}
            placeholder="leave empty to use the tier"
            className="h-7! px-2! font-mono text-[11px]!"
          />
          {localModels.length > 0 && (
            <div className="mt-1.5">
              <span className="text-[10px] text-fg-muted">
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
                        ? "border-accent bg-accent-subtle text-accent-fg"
                        : "border-border text-fg-muted hover:border-accent/50"
                    }`}
                  >
                    {m.name}
                  </button>
                ))}
              </div>
              {/* Said once, here, because "why is Ollama not in Run on"
                  is the question this layout provokes. */}
              <p className="mt-1 text-[10px] leading-relaxed text-fg-muted">
                Served by Ollama or LM Studio and run through OpenCode — pick that engine
                above.
              </p>
            </div>
          )}
          {modelId.trim() !== "" && (
            <span className="mt-1 block text-[10px] text-warning-fg">
              This chat runs on {modelId.trim()}, ignoring the tier.
            </span>
          )}
        </div>
      </Row>
      <Row label="Thinking">
        <EffortPicker value={effort} onChange={onEffort} />
      </Row>
      <p className="text-[11px] text-fg-muted">
        Sticks to this conversation, not just the next message.
      </p>
    </Popover>
  );
}

function Row({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
        {label}
      </div>
      {children}
    </div>
  );
}
