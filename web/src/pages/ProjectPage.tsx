import { useCallback, useEffect, useState } from "react";
import { useParams } from "react-router-dom";
import { AnimatePresence } from "framer-motion";
import { api, Project, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { Board } from "../components/Board";
import { NewTaskModal } from "../components/NewTaskModal";
import { TaskDrawer } from "../components/TaskDrawer";
import { ChatPanel } from "../components/chat/ChatPanel";

export default function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  const { active } = useWorkspace();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [selected, setSelected] = useState<Task | null>(null);

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
          <span className="rounded-full bg-panel-2 px-2 py-0.5 text-xs text-ink-dim">
            Tasks Board
          </span>
          <button
            onClick={() => setShowNew(true)}
            className="ml-auto rounded-lg bg-accent px-3.5 py-1.5 text-sm font-medium text-white hover:opacity-90"
          >
            + New task
          </button>
        </header>

        <div className="min-h-0 flex-1">
          <Board tasks={tasks} onSelect={setSelected} />
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
          />
        )}
      </AnimatePresence>
    </div>
  );
}
