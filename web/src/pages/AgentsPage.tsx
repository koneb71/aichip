import { useCallback, useEffect, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { AgentEditorDrawer } from "../components/agents/AgentEditorDrawer";
import { GenerateWizard } from "../components/agents/GenerateWizard";
import { Card, Empty, Item, Page, PageHead, Stagger } from "../components/ui/Surface";
import { Icon } from "../components/ui/Icon";
import { Button } from "../components/ui/Button";
import { Plus, Sparkles } from "lucide-react";
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
    api.allAgents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
  }, [active]);

  useEffect(refresh, [refresh]);

  // `?new=1` (the top bar's New menu) and `?agent=<id>` (a palette hit) open
  // the editor; the param is consumed so a reload does not reopen it.
  const [params, setParams] = useSearchParams();
  useEffect(() => {
    const want = params.get("agent");
    if (params.get("new") === "1") {
      setEditing("new");
      setParams((p) => {
        p.delete("new");
        return p;
      }, { replace: true });
    } else if (want) {
      const found = agents.find((a) => a.id === want);
      if (found) {
        setEditing(found);
        setParams((p) => {
          p.delete("agent");
          return p;
        }, { replace: true });
      }
    }
  }, [params, agents, setParams]);

  const working = agents.filter((a) => a.status !== "retired");
  const retired = agents.filter((a) => a.status === "retired");

  return (
    <Page>
      <PageHead
        title="Agents"
        subtitle="Reusable specialists you can bind to tasks — or let the assistant pick from."
        actions={
          <>
            <Button size="sm" icon={<Sparkles className="size-3.5" />} onClick={() => setWizard(true)}>
              Generate with AI
            </Button>
            <Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setEditing("new")}>
              New agent
            </Button>
          </>
        }
      />

      <Stagger className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {working.map((a) => (
          <Item key={a.id}>
            <Card onClick={() => setEditing(a)} className="h-full p-4">
              <div className="flex items-center gap-3">
                <span
                  className="grid size-10 shrink-0 place-items-center rounded-xl text-sm font-bold text-on-accent transition-transform duration-300 group-hover:scale-105"
                  style={{
                    background: a.color,
                    boxShadow: `0 4px 12px -4px ${a.color}`,
                  }}
                >
                  {a.name.slice(0, 1).toUpperCase()}
                </span>
                <div className="min-w-0">
                  <div className="flex items-center gap-1.5">
                    <span className="truncate text-sm font-semibold">{a.name}</span>
                    {a.status === "paused" && (
                      <span
                        className="shrink-0 rounded-full bg-warning-subtle px-1.5 py-0.5 text-[10px] font-medium text-warning-fg"
                        title={a.pauseReason ?? "Starts nothing until resumed"}
                      >
                        paused
                      </span>
                    )}
                  </div>
                  <span
                    className="mt-0.5 inline-block rounded-full px-2 py-0.5 text-[11px] font-medium"
                    style={{ background: tierSoft[a.modelTier], color: tierColor[a.modelTier] }}
                  >
                    {tierModel(a.modelTier)}
                  </span>
                </div>
              </div>
              <p className="mt-3 line-clamp-2 text-xs leading-relaxed text-fg-muted">
                {a.description || "No description yet."}
              </p>
            </Card>
          </Item>
        ))}
        {working.length === 0 && (
          <div className="col-span-full">
            <Empty
              icon={<Icon name="agents" size={28} />}
              title="No agents yet"
              hint="Generate a starter set with AI, or create one by hand. An agent is who does the work; a skill is how."
            />
          </div>
        )}
      </Stagger>

      {retired.length > 0 && (
        <details className="mt-8">
          <summary className="cursor-pointer text-xs font-semibold uppercase tracking-wider text-fg-muted">
            Retired · {retired.length}
          </summary>
          <p className="mt-1 text-[11px] text-fg-muted">
            No new work and gone from pickers; their runs and comments still name them.
          </p>
          <div className="mt-3 flex flex-wrap gap-2">
            {retired.map((a) => (
              <button
                key={a.id}
                onClick={() => setEditing(a)}
                className="flex items-center gap-2 rounded-lg border border-border px-2.5 py-1.5 text-xs text-fg-muted hover:bg-panel-2"
              >
                <span className="size-2 rounded-full opacity-50" style={{ background: a.color }} />
                {a.name}
              </button>
            ))}
          </div>
        </details>
      )}

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
