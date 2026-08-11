import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { AgentEditorDrawer } from "../components/agents/AgentEditorDrawer";
import { GenerateWizard } from "../components/agents/GenerateWizard";
import { Card, Empty, Item, Page, PageHead, Stagger } from "../components/ui/Surface";
import { Icon } from "../components/ui/Icon";
import { tappable } from "../lib/motion";
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
    <Page>
      <PageHead
        title="Agents"
        subtitle="Reusable specialists you can bind to tasks — or let the assistant pick from."
        actions={
          <>
            <motion.button
              {...tappable}
              onClick={() => setWizard(true)}
              className="ring-focus flex items-center gap-1.5 rounded-xl border border-accent/30 bg-accent/[0.06] px-3.5 py-2 text-sm font-medium text-accent transition-colors hover:bg-accent/10"
            >
              <Icon name="sparkle" size={15} />
              Generate with AI
            </motion.button>
            <motion.button
              {...tappable}
              onClick={() => setEditing("new")}
              className="ring-focus flex items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
            >
              <Icon name="plus" size={15} strokeWidth={2.5} />
              New agent
            </motion.button>
          </>
        }
      />

      <Stagger className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {agents.map((a) => (
          <Item key={a.id}>
            <Card onClick={() => setEditing(a)} className="h-full p-4">
              <div className="flex items-center gap-3">
                <span
                  className="grid size-10 shrink-0 place-items-center rounded-xl text-sm font-bold text-white transition-transform duration-300 group-hover:scale-105"
                  style={{
                    background: a.color,
                    boxShadow: `0 4px 12px -4px ${a.color}`,
                  }}
                >
                  {a.name.slice(0, 1).toUpperCase()}
                </span>
                <div className="min-w-0">
                  <div className="truncate text-sm font-semibold">{a.name}</div>
                  <span
                    className="mt-0.5 inline-block rounded-full px-2 py-0.5 text-[11px] font-medium"
                    style={{ background: tierSoft[a.modelTier], color: tierColor[a.modelTier] }}
                  >
                    {tierModel(a.modelTier)}
                  </span>
                </div>
              </div>
              <p className="mt-3 line-clamp-2 text-xs leading-relaxed text-ink-dim">
                {a.description || "No description yet."}
              </p>
            </Card>
          </Item>
        ))}
        {agents.length === 0 && (
          <div className="col-span-full">
            <Empty
              icon={<Icon name="agents" size={28} />}
              title="No agents yet"
              hint="Generate a starter set with AI, or create one by hand. An agent is who does the work; a skill is how."
            />
          </div>
        )}
      </Stagger>

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
    </Page>
  );
}
