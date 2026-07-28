import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Agent, AgentMemory, api, Effort, McpServer, Tier, tierColor, tierModel } from "../../lib/api";

const TIERS: Tier[] = ["easy", "medium", "complex"];
const EFFORTS: Effort[] = ["low", "medium", "high", "xhigh", "max"];
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
  const [effort, setEffort] = useState<Effort | "">(agent?.effort ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Which connected MCP servers this agent may use. Opt-in per agent: a
  // frontend specialist has no business holding a production database
  // connection just because the workspace has one configured.
  const [servers, setServers] = useState<McpServer[]>([]);
  const [enabledServers, setEnabledServers] = useState<string[]>([]);

  useEffect(() => {
    api.mcpServers(workspaceId).then((r) => setServers(r.servers)).catch(() => {});
    if (agent) {
      api
        .agentMcpServers(agent.id)
        .then((r) => setEnabledServers(r.serverIds))
        .catch(() => {});
    }
  }, [workspaceId, agent]);

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
      effort: effort || null,
    };
    try {
      // A new agent has no id until it exists, so the server list is saved
      // second either way.
      const saved = agent ? await api.updateAgent(agent.id, body) : await api.createAgent(body);
      if (servers.length > 0) {
        await api.setAgentMcpServers(saved.id, enabledServers);
      }
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
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-full max-w-[480px] flex-col border-l border-line bg-panel"
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
        <Field label="Thinking">
          <div className="flex gap-1.5">
            <button
              onClick={() => setEffort("")}
              className={`rounded-lg border px-2 py-1.5 text-xs ${
                effort === "" ? "border-accent text-accent" : "border-line text-ink-dim"
              }`}
            >
              default
            </button>
            {EFFORTS.map((e) => (
              <button
                key={e}
                onClick={() => setEffort(e)}
                className={`flex-1 rounded-lg border px-1 py-1.5 text-xs ${
                  effort === e ? "border-accent text-accent" : "border-line text-ink-dim"
                }`}
              >
                {e}
              </button>
            ))}
          </div>
          <div className="mt-1 text-[11px] text-ink-dim">
            How hard this agent thinks before answering. Separate from the model —
            more thinking is usually cheaper than a bigger model.
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
        {servers.length > 0 && (
          <Field label="Connections">
            <div className="space-y-1.5">
              {servers.map((s) => (
                <label
                  key={s.id}
                  className="flex cursor-pointer items-start gap-2 rounded-lg border border-line p-2.5 hover:bg-panel-2"
                >
                  <input
                    type="checkbox"
                    checked={enabledServers.includes(s.id)}
                    onChange={(e) =>
                      setEnabledServers((prev) =>
                        e.target.checked
                          ? [...prev, s.id]
                          : prev.filter((id) => id !== s.id),
                      )
                    }
                    className="mt-0.5 accent-[var(--color-accent)]"
                  />
                  <span className="min-w-0 text-xs">
                    <span className="font-medium">{s.name}</span>
                    <span className="mt-0.5 block truncate text-ink-dim">
                      {s.transport === "stdio"
                        ? [s.command, ...s.args].join(" ")
                        : s.url}
                    </span>
                  </span>
                </label>
              ))}
            </div>
            <div className="mt-1.5 text-[11px] text-ink-dim">
              Tools from these servers become available to this agent on every run.
            </div>
          </Field>
        )}
        {error && (
          <div className="rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}
        {agent && <MemorySection agentId={agent.id} />}
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

// What the agent remembers — written automatically as it completes tasks and
// answers mentions, injected into its next runs. The user can prune it.
function MemorySection({ agentId }: { agentId: string }) {
  const [memories, setMemories] = useState<AgentMemory[]>([]);

  const refresh = useCallback(
    () => api.agentMemories(agentId).then((r) => setMemories(r.memories)).catch(() => {}),
    [agentId],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  return (
    <Field label={`Memory (${memories.length})`}>
      {memories.length === 0 ? (
        <div className="text-xs text-ink-dim">
          Nothing yet — memories appear as this agent completes tasks and
          answers mentions, and are fed into its next runs.
        </div>
      ) : (
        <div className="flex max-h-56 flex-col gap-1.5 overflow-y-auto">
          {memories.map((m) => (
            <div
              key={m.id}
              className="group flex items-start gap-2 rounded-lg border border-line bg-panel-2 px-2.5 py-1.5 text-xs"
            >
              <div className="min-w-0 flex-1">
                <div className="text-[10px] text-ink-dim">
                  {new Date(m.ts).toLocaleDateString()} · {m.projectName ?? "all projects"} ·{" "}
                  {m.kind.replace("_", " ")}
                </div>
                <div className="mt-0.5 break-words">{m.content}</div>
              </div>
              <button
                onClick={() => api.forgetMemory(m.id).then(refresh)}
                title="Forget"
                className="shrink-0 text-ink-dim opacity-0 hover:text-danger group-hover:opacity-100"
              >
                ✕
              </button>
            </div>
          ))}
        </div>
      )}
    </Field>
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
