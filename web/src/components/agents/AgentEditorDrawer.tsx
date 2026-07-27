import { useState } from "react";
import { motion } from "framer-motion";
import { Agent, api, Tier, tierColor, tierModel } from "../../lib/api";

const TIERS: Tier[] = ["easy", "medium", "complex"];
const COLORS = ["#4f46e5", "#059669", "#c026d3", "#ea580c", "#0284c7", "#dc2626"];

export function AgentEditorDrawer({
  workspaceId,
  agent,
  onClose,
  onChanged,
}: {
  workspaceId: string;
  agent: Agent | null;
  onClose: () => void;
  onChanged: () => void;
}) {
  const [name, setName] = useState(agent?.name ?? "");
  const [description, setDescription] = useState(agent?.description ?? "");
  const [systemPrompt, setSystemPrompt] = useState(agent?.systemPrompt ?? "");
  const [tier, setTier] = useState<Tier>(agent?.modelTier ?? "medium");
  const [color, setColor] = useState(agent?.color ?? COLORS[0]);
  const [preset, setPreset] = useState(agent?.permissionPreset ?? "reviewed");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const save = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    const body = {
      workspace_id: workspaceId,
      name: name.trim(),
      description,
      system_prompt: systemPrompt,
      model_tier: tier,
      color,
      permission_preset: preset,
    };
    try {
      if (agent) await api.updateAgent(agent.id, body);
      else await api.createAgent(body);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!agent) return;
    await api.deleteAgent(agent.id);
    onChanged();
  };

  return (
    <motion.aside
      initial={{ x: 480 }}
      animate={{ x: 0 }}
      exit={{ x: 480 }}
      transition={{ type: "spring", stiffness: 320, damping: 34 }}
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-[480px] flex-col border-l border-line bg-panel"
    >
      <div className="flex items-center justify-between border-b border-line p-5">
        <div className="text-base font-semibold">
          {agent ? `Edit ${agent.name}` : "New agent"}
        </div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">
          ✕
        </button>
      </div>

      <div className="min-h-0 flex-1 space-y-4 overflow-y-auto p-5">
        <Field label="Name">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            className="w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
          />
        </Field>
        <Field label="Color">
          <div className="flex gap-2">
            {COLORS.map((c) => (
              <button
                key={c}
                onClick={() => setColor(c)}
                className="h-7 w-7 rounded-full border-2"
                style={{ background: c, borderColor: color === c ? "var(--color-ink)" : "transparent" }}
              />
            ))}
          </div>
        </Field>
        <Field label="Description">
          <input
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            className="w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
          />
        </Field>
        <Field label="System prompt">
          <textarea
            value={systemPrompt}
            onChange={(e) => setSystemPrompt(e.target.value)}
            rows={7}
            className="w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
            placeholder="Role, approach, output standards…"
          />
        </Field>
        <Field label="Model tier">
          <div className="flex gap-2">
            {TIERS.map((t) => (
              <button
                key={t}
                onClick={() => setTier(t)}
                className="flex-1 rounded-lg border px-3 py-2 text-sm capitalize"
                style={{
                  borderColor: tier === t ? tierColor[t] : "var(--color-line)",
                  color: tier === t ? tierColor[t] : "var(--color-ink-dim)",
                }}
              >
                {t}
                <span className="block text-[11px] opacity-75">{tierModel[t]}</span>
              </button>
            ))}
          </div>
        </Field>
        <Field label="Permissions">
          <div className="flex gap-2">
            {[
              ["reviewed", "Reviewed"],
              ["auto_edit", "Auto-edit"],
            ].map(([value, label]) => (
              <button
                key={value}
                onClick={() => setPreset(value)}
                className={`flex-1 rounded-lg border px-3 py-2 text-sm ${
                  preset === value
                    ? "border-accent text-accent"
                    : "border-line text-ink-dim"
                }`}
              >
                {label}
              </button>
            ))}
          </div>
        </Field>
        {error && (
          <div className="rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}
      </div>

      <div className="flex items-center justify-between border-t border-line p-4">
        {agent && !agent.builtin ? (
          <button onClick={remove} className="text-sm text-danger hover:underline">
            Delete
          </button>
        ) : (
          <span />
        )}
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={save}
          disabled={busy || !name.trim()}
          className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
        >
          {busy ? "Saving…" : "Save agent"}
        </motion.button>
      </div>
    </motion.aside>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div className="mb-1.5 text-xs font-semibold uppercase tracking-wide text-ink-dim">
        {label}
      </div>
      {children}
    </div>
  );
}
