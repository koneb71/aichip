import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { AnimatePresence } from "framer-motion";
import { api, Project, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { Board } from "../components/Board";
import { NewTaskModal } from "../components/NewTaskModal";
import { TaskDrawer } from "../components/TaskDrawer";
import { ChatPanel } from "../components/chat/ChatPanel";
import { WorkflowsPanel } from "../components/workflows/WorkflowsPanel";
import { OrgRunView } from "../components/orgs/OrgRunView";

export default function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  const { active } = useWorkspace();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [selected, setSelected] = useState<Task | null>(null);
  const [tab, setTab] = useState<"board" | "workflows">("board");
  const [teamRoom, setTeamRoom] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!projectId) return;
    const t = await api.tasks({ projectId });
    setTasks(t.tasks);
  }, [projectId]);

  useEffect(() => {
    if (!active || !projectId) return;
    api
      .projects(active.id)
      .then(({ projects }) => setProject(projects.find((p) => p.id === projectId) ?? null))
      .catch(() => {});
  }, [active, projectId]);

  useEffect(() => {
    refresh().catch(() => {});
    const interval = setInterval(() => refresh().catch(() => {}), 2500);
    return () => clearInterval(interval);
  }, [refresh]);

  if (!projectId) return null;

  return (
    <div className="grid h-full grid-cols-[380px_1fr]">
      <ChatPanel projectId={projectId} />

      <div className="flex min-h-0 min-w-0 flex-col">
        <header className="flex items-center gap-3 border-b border-line bg-panel px-6 py-3">
          <div className="text-base font-semibold">{project?.name ?? "Project"}</div>
          <div className="flex gap-1 rounded-lg bg-panel-2 p-0.5">
            {(["board", "workflows"] as const).map((t) => (
              <button
                key={t}
                onClick={() => setTab(t)}
                className={`rounded-md px-3 py-1 text-xs capitalize transition-colors ${
                  tab === t ? "bg-panel font-medium text-ink shadow-sm" : "text-ink-dim"
                }`}
              >
                {t === "board" ? "Tasks Board" : "Workflows"}
              </button>
            ))}
          </div>
          {tab === "board" && (
            <button
              onClick={() => setShowNew(true)}
              className="ml-auto rounded-lg bg-accent px-3.5 py-1.5 text-sm font-medium text-white hover:opacity-90"
            >
              + New task
            </button>
          )}
        </header>

        <div className="min-h-0 flex-1">
          {tab === "board" ? (
            <Board tasks={tasks} onSelect={setSelected} />
          ) : (
            <WorkflowsPanel projectId={projectId} />
          )}
        </div>
      </div>

      <AnimatePresence>
        {showNew && project && (
          <NewTaskModal
            project={project}
            onClose={() => setShowNew(false)}
            onCreated={() => {
              setShowNew(false);
              refresh();
            }}
          />
        )}
        {selected && (
          <TaskDrawer
            task={tasks.find((t) => t.id === selected.id) ?? selected}
            onClose={() => setSelected(null)}
            onChanged={refresh}
            onOpenTeamRoom={setTeamRoom}
          />
        )}
        {teamRoom && (
          <OrgRunView runId={teamRoom} onClose={() => setTeamRoom(null)} />
        )}
      </AnimatePresence>
    </div>
  );
}
