import { useCallback, useEffect, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
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
import { NARROW, useMediaQuery } from "../lib/useMediaQuery";
import { PreviewsPanel } from "../components/previews/PreviewsPanel";

const TABS = [
  { key: "board", label: "Tasks Board" },
  { key: "workflows", label: "Workflows" },
  { key: "files", label: "Files" },
  { key: "previews", label: "Previews" },
  // Docked beside the board on a wide screen; below `lg` there is no room for
  // a 380px column, so the chat becomes a tab like the others.
  { key: "chat", label: "Chat", narrowOnly: true },
] as const;

type Tab = (typeof TABS)[number]["key"];

export default function ProjectPage() {
  const { projectId } = useParams<{ projectId: string }>();
  const { active } = useWorkspace();
  const [project, setProject] = useState<Project | null>(null);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [showNew, setShowNew] = useState(false);
  const [tab, setTab] = useState<Tab>("board");
  const [teamRoom, setTeamRoom] = useState<string | null>(null);
  const narrow = useMediaQuery(NARROW);

  // Which card is open lives in the URL, not in state.
  //
  // That makes a card addressable — the knowledge base links straight to one,
  // and back/forward and a pasted link all work. Deriving the open card from
  // the URL rather than mirroring it into state is deliberate: the board
  // refreshes every 2.5s, and two sources of truth for "what is open" is how you
  // get a drawer that reopens itself after you close it.
  const [params, setParams] = useSearchParams();
  const selected = tasks.find((t) => t.id === params.get("task")) ?? null;
  const openTask = useCallback(
    (task: Task | null) =>
      setParams(
        (prev) => {
          const next = new URLSearchParams(prev);
          if (task) next.set("task", task.id);
          else next.delete("task");
          return next;
        },
        // Opening a card is not a place you should have to press Back out of
        // twice — it replaces the entry rather than stacking one per click.
        { replace: true },
      ),
    [setParams],
  );

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

  // The chat tab only exists while it has nowhere to dock, so a viewport that
  // widens while it is selected must not leave the page on a dead tab.
  const activeTab: Tab = tab === "chat" && !narrow ? "board" : tab;

  return (
    <div className="grid h-full grid-cols-[minmax(0,1fr)] lg:grid-cols-[380px_minmax(0,1fr)]">
      {!narrow && <ChatPanel projectId={projectId} />}

      <div className="flex min-h-0 min-w-0 flex-col">
        <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-line bg-panel px-4 py-3 lg:px-6">
          <div className="truncate text-base font-semibold">{project?.name ?? "Project"}</div>
          {project?.vcs === "none" && (
            <span
              title={project.vcsNote ?? undefined}
              className="rounded-full bg-amber-50 px-2 py-0.5 text-[11px] text-amber-700"
            >
              edits in place
            </span>
          )}
          {project && <AutonomyToggle project={project} onChanged={setProject} />}
          <div className="flex min-w-0 gap-1 overflow-x-auto rounded-lg bg-panel-2 p-0.5">
            {TABS.filter((t) => narrow || !("narrowOnly" in t)).map((t) => (
              <button
                key={t.key}
                onClick={() => setTab(t.key)}
                className={`shrink-0 rounded-md px-3 py-1 text-xs transition-colors ${
                  activeTab === t.key ? "bg-panel font-medium text-ink shadow-sm" : "text-ink-dim"
                }`}
              >
                {t.label}
              </button>
            ))}
          </div>
          {activeTab === "board" && (
            <button
              onClick={() => setShowNew(true)}
              className="ml-auto shrink-0 rounded-lg bg-accent px-3.5 py-1.5 text-sm font-medium text-white hover:opacity-90"
            >
              + New task
            </button>
          )}
        </header>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {activeTab === "board" && (
            <>
              {moveError && (
                <div className="mx-4 mt-3 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger lg:mx-5">
                  {moveError}
                </div>
              )}
              <Board tasks={tasks} onSelect={openTask} onMove={move} />
            </>
          )}
          {activeTab === "workflows" && <WorkflowsPanel projectId={projectId} />}
          {activeTab === "files" && <FilesPanel projectId={projectId} />}
          {activeTab === "previews" && <PreviewsPanel projectId={projectId} />}
          {activeTab === "chat" && <ChatPanel projectId={projectId} />}
        </div>
      </div>

      {/* Keyed because AnimatePresence tracks children by key, and these three
          conditional siblings appear and disappear independently — two can be
          on screen at once. Unkeyed, it has only child order to go on.

          The drawer's key is constant rather than the task id, so opening a
          different card is a prop change that keeps the panel's scroll and tab
          where they were, instead of tearing it down and sliding a new one in. */}
      <AnimatePresence>
        {showNew && project && (
          <NewTaskModal
            key="new-task"
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
            onOpenPreviews={() => setTab("previews")}
            key="task-drawer"
            task={selected}
            workspaceId={project?.workspaceId ?? ""}
            onClose={() => openTask(null)}
            onChanged={refresh}
            onOpenTeamRoom={setTeamRoom}
            boardTasks={tasks}
            onOpenTask={openTask}
          />
        )}
        {teamRoom && (
          <OrgRunView key="team-room" runId={teamRoom} onClose={() => setTeamRoom(null)} />
        )}
      </AnimatePresence>
    </div>
  );
}

/**
 * Whether agents may work in this project without stopping to ask.
 *
 * The orchestrator has always consulted `full_auto_opt_in` before honouring
 * a no-prompts run, but nothing could ever set it — so every run asked about
 * everything and there was no way to say "just get on with it". This is that
 * switch.
 *
 * Only offered for git projects: the reason skipping prompts is reasonable at
 * all is that the work happens in an isolated worktree you read as a diff
 * before it touches your branch. A project that edits in place has neither.
 */
function AutonomyToggle({
  project,
  onChanged,
}: {
  project: Project;
  onChanged: (p: Project) => void;
}) {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (project.vcs !== "git") return null;

  const toggle = async () => {
    setBusy(true);
    setError(null);
    try {
      onChanged(await api.setProjectFullAuto(project.id, !project.fullAutoOptIn));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <button
      onClick={toggle}
      disabled={busy}
      title={
        error ??
        (project.fullAutoOptIn
          ? "Agents may work straight through here — unless the agent on a card carries its own permission setting, which wins. Each card shows which applies. Click to make them ask again."
          : "Agents stop to ask before edits and commands. Click to let them work uninterrupted — the run stays in an isolated worktree you review.")
      }
      className={`rounded-full px-2 py-0.5 text-[11px] transition-colors disabled:opacity-50 ${
        project.fullAutoOptIn
          ? "bg-tier-easy-soft text-tier-easy"
          : "bg-panel-2 text-ink-dim hover:text-ink"
      }`}
    >
      {/* "allows" rather than "works": this unlocks working without asking, it
          does not guarantee it. An agent's own preset outranks the project, so
          the unqualified promise this used to make was one it could not keep. */}
      {project.fullAutoOptIn ? "✓ allows working without asking" : "asks before acting"}
    </button>
  );
}
