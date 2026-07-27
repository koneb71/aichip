import { useEffect, useState } from "react";
import { Agent, Tier, tierColor, tierModel } from "../../lib/api";
import { StepData, renameStep, uniqueStepId } from "../../lib/workflowGraph";

const TIERS: Tier[] = ["easy", "medium", "complex"];

export function StepInspector({
  step,
  steps,
  agents,
  onChange,
  onDelete,
}: {
  step: StepData;
  steps: StepData[];
  agents: Agent[];
  onChange: (steps: StepData[]) => void;
  onDelete: () => void;
}) {
  // The id is edited as free text but only committed when it's valid and
  // unique, so half-typed names don't orphan dependency links.
  const [draftId, setDraftId] = useState(step.id);
  useEffect(() => setDraftId(step.id), [step.id]);

  const patch = (changes: Partial<StepData>) =>
    onChange(steps.map((s) => (s.id === step.id ? { ...s, ...changes } : s)));

  const commitId = () => {
    const cleaned = draftId.trim().replace(/[^A-Za-z0-9_-]+/g, "_");
    if (!cleaned || cleaned === step.id) return setDraftId(step.id);
    const others = steps.filter((s) => s.id !== step.id);
    const unique = uniqueStepId(others, cleaned);
    onChange(renameStep(steps, step.id, unique));
    setDraftId(unique);
  };

  const parallel = step.parallel ?? 1;

  return (
    <div className="flex h-full flex-col overflow-y-auto border-l border-line bg-panel">
      <div className="flex items-center justify-between border-b border-line px-4 py-3">
        <span className="text-sm font-semibold">Step</span>
        <button onClick={onDelete} className="text-xs text-danger hover:underline">
          Delete
        </button>
      </div>

      <div className="space-y-4 p-4">
        <Field label="ID" hint="Referenced by other steps and in {{ }} templates">
          <input
            value={draftId}
            onChange={(e) => setDraftId(e.target.value)}
            onBlur={commitId}
            onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
            className="w-full rounded-lg border border-line px-2.5 py-1.5 font-mono text-sm outline-none focus:border-accent"
          />
        </Field>

        <Field label="Prompt">
          <textarea
            value={step.prompt}
            onChange={(e) => patch({ prompt: e.target.value })}
            rows={8}
            placeholder="What should this step do?"
            className="w-full resize-none rounded-lg border border-line px-2.5 py-1.5 text-sm outline-none focus:border-accent"
          />
          {steps.length > 1 && (
            <InsertOutput steps={steps} current={step} onInsert={(t) => patch({ prompt: step.prompt + t })} />
          )}
        </Field>

        <Field label="Model">
          <div className="flex gap-1.5">
            {TIERS.map((t) => (
              <button
                key={t}
                onClick={() => patch({ model: step.model === t ? undefined : t })}
                className="flex-1 rounded-lg border px-2 py-1.5 text-xs capitalize"
                style={{
                  borderColor: step.model === t ? tierColor[t] : "var(--color-line)",
                  color: step.model === t ? tierColor[t] : "var(--color-ink-dim)",
                }}
              >
                {t}
                <span className="block text-[10px] opacity-70">{tierModel[t]}</span>
              </button>
            ))}
          </div>
        </Field>

        <Field label="Agent" hint="Supplies the system prompt and tools">
          <select
            value={step.agent ?? ""}
            onChange={(e) => patch({ agent: e.target.value || undefined })}
            className="w-full rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm"
          >
            <option value="">None</option>
            {agents.map((a) => (
              <option key={a.id} value={a.name}>
                {a.name}
              </option>
            ))}
          </select>
        </Field>

        <Field label="Parallel attempts" hint="More than one turns this into a fan-out">
          <div className="flex items-center gap-2">
            <input
              type="range"
              min={1}
              max={8}
              value={parallel}
              onChange={(e) => {
                const n = Number(e.target.value);
                patch({
                  parallel: n > 1 ? n : undefined,
                  isolatedWorktrees: n > 1 ? step.isolatedWorktrees ?? true : false,
                });
              }}
              className="flex-1 accent-[var(--color-accent)]"
            />
            <span className="w-6 text-center font-mono text-sm">{parallel}</span>
          </div>
          {parallel > 1 && (
            <Toggle
              label="Give each attempt its own worktree"
              checked={step.isolatedWorktrees ?? false}
              onChange={(v) => patch({ isolatedWorktrees: v })}
            />
          )}
        </Field>

        {step.needs.length > 0 && (
          <Field label="Runs after" hint="Drag between node handles to change">
            <div className="flex flex-wrap gap-1">
              {step.needs.map((n) => (
                <span
                  key={n}
                  className="flex items-center gap-1 rounded-full bg-panel-2 px-2 py-0.5 font-mono text-[11px]"
                >
                  {n}
                  <button
                    onClick={() => patch({ needs: step.needs.filter((x) => x !== n) })}
                    className="text-ink-dim hover:text-danger"
                  >
                    ✕
                  </button>
                </span>
              ))}
            </div>
            <Toggle
              label="Continue the previous step's session"
              checked={step.session === "continue"}
              onChange={(v) => patch({ session: v ? "continue" : undefined })}
            />
          </Field>
        )}
      </div>
    </div>
  );
}

function InsertOutput({
  steps,
  current,
  onInsert,
}: {
  steps: StepData[];
  current: StepData;
  onInsert: (text: string) => void;
}) {
  const candidates = steps.filter((s) => s.id !== current.id);
  return (
    <div className="mt-1.5 flex flex-wrap items-center gap-1">
      <span className="text-[11px] text-ink-dim">Insert output:</span>
      {candidates.map((s) => {
        const many = (s.parallel ?? 1) > 1;
        const token = `{{ steps.${s.id}.${many ? "outputs" : "output"} }}`;
        return (
          <button
            key={s.id}
            onClick={() => onInsert(token)}
            title={token}
            className="rounded-full border border-line px-1.5 py-0.5 font-mono text-[10px] hover:border-accent hover:text-accent"
          >
            {s.id}
          </button>
        );
      })}
    </div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
        {label}
      </div>
      {children}
      {hint && <div className="mt-1 text-[11px] text-ink-dim/80">{hint}</div>}
    </div>
  );
}

function Toggle({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <label className="mt-2 flex cursor-pointer items-center gap-2 text-xs text-ink-dim">
      <input
        type="checkbox"
        checked={checked}
        onChange={(e) => onChange(e.target.checked)}
        className="accent-[var(--color-accent)]"
      />
      {label}
    </label>
  );
}
