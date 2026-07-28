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
import { FilesPanel } from "../components/files/FilesPanel";
import { OrgRunView } from "../components/orgs/OrgRunView";

const TABS = [
  { key: "board", label: "Tasks Board" },
  { key: "workflows", label: "Workflows" },
  { key: "files", label: "Files" },
] as const;

type Tab = (typeof TABS)[number]["key"];

export default function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  const { active } = useWorkspace();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [selected, setSelected] = useState<Task | null>(null);
  const [tab, setTab] = useState<Tab>("board");
  const [teamRoom, setTeamRoom] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (!projectId) return;
    const t = await api.tasks({ projectId });
    setTasks(t.tasks);
  }, [projectId]);

  const [moveError, setMoveError] = useState<string | null>(null);
  const move = useCallback(
    async (taskId: string, column: Task["boardColumn"], position: number) => {
      // Optimistic: the card lands where it was dropped, then the server
      // refresh either confirms it or puts it back (409 while a run is live).
      setTasks((prev) =>
        prev.map((t) => (t.id === taskId ? { ...t, boardColumn: column, position } : t)),
      );
      try {
        setMoveError(null);
        await api.moveTask(taskId, { board_column: column, position });
      } catch (e) {
        setMoveError(String(e));
        setTimeout(() => setMoveError(null), 5000);
      }
      refresh().catch(() => {});
    },
    [refresh],
  );

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
          {project?.vcs === "none" && (
            <span
              title={project.vcsNote ?? undefined}
              className="rounded-full bg-amber-50 px-2 py-0.5 text-[11px] text-amber-700"
            >
              edits in place
            </span>
          )}
          <div className="flex gap-1 rounded-lg bg-panel-2 p-0.5">
            {TABS.map((t) => (
              <button
                key={t.key}
                onClick={() => setTab(t.key)}
                className={`rounded-md px-3 py-1 text-xs transition-colors ${
                  tab === t.key ? "bg-panel font-medium text-ink shadow-sm" : "text-ink-dim"
                }`}
              >
                {t.label}
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
          {tab === "board" && (
            <>
              {moveError && (
                <div className="mx-5 mt-3 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">
                  {moveError}
                </div>
              )}
              <Board tasks={tasks} onSelect={setSelected} onMove={move} />
            </>
          )}
          {tab === "workflows" && <WorkflowsPanel projectId={projectId} />}
          {tab === "files" && <FilesPanel projectId={projectId} />}
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
