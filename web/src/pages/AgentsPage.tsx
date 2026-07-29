import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { AgentEditorDrawer } from "../components/agents/AgentEditorDrawer";
import { GenerateWizard } from "../components/agents/GenerateWizard";
import { tierColor, tierSoft } from "../lib/api";
import { useTierModel } from "../lib/models";

export default function AgentsPage() {
  const tierModel = useTierModel();
  const { active } = useWorkspace();
  const [agents, setAgents] = useState<Agent[]>([]);
  const [editing, setEditing] = useState<Agent | "new" | null>(null);
  const [wizard, setWizard] = useState(false);

  const refresh = useCallback(() => {
    if (!active) return;
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
  }, [active]);

  useEffect(refresh, [refresh]);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Agents</h1>
          <p className="mt-0.5 text-sm text-ink-dim">
            Reusable specialists you can bind to tasks — or let the assistant pick from.
          </p>
        </div>
        <div className="flex gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setWizard(true)}
            className="rounded-lg border border-accent px-4 py-1.5 text-sm font-medium text-accent"
          >
            ✦ Generate with AI
          </motion.button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setEditing("new")}
            className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white"
          >
            + New agent
          </motion.button>
        </div>
      </div>

      <div className="mt-6 grid max-w-5xl grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {agents.map((a) => (
          <motion.button
            key={a.id}
            layout
            whileHover={{ y: -2 }}
            onClick={() => setEditing(a)}
            className="card-shadow rounded-xl border border-line bg-panel p-4 text-left"
          >
            <div className="flex items-center gap-3">
              <span
                className="flex h-9 w-9 items-center justify-center rounded-lg text-sm font-bold text-white"
                style={{ background: a.color }}
              >
                {a.name.slice(0, 1).toUpperCase()}
              </span>
              <div className="min-w-0">
                <div className="truncate text-sm font-semibold">{a.name}</div>
                <span
                  className="rounded-full px-2 py-0.5 text-[11px]"
                  style={{ background: tierSoft[a.modelTier], color: tierColor[a.modelTier] }}
                >
                  {tierModel(a.modelTier)}
                </span>
              </div>
            </div>
            <p className="mt-3 line-clamp-2 text-xs text-ink-dim">
              {a.description || "No description yet."}
            </p>
          </motion.button>
        ))}
        {agents.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
            No agents yet. Generate a starter set with AI, or create one by hand.
          </div>
        )}
      </div>

      <AnimatePresence>
        {editing && active && (
          <AgentEditorDrawer
            workspaceId={active.id}
            agent={editing === "new" ? null : editing}
            onClose={() => setEditing(null)}
            onChanged={() => {
              setEditing(null);
              refresh();
            }}
          />
        )}
        {wizard && active && (
          <GenerateWizard
            workspaceId={active.id}
            onClose={() => setWizard(false)}
            onSaved={refresh}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
