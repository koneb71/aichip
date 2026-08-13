import { lazy, Suspense, useCallback, useEffect, useState } from "react";
import { useParams, useSearchParams } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { api, Project, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { Board } from "../components/Board";
import { GitSync } from "../components/GitSync";
import { NewTaskModal } from "../components/NewTaskModal";
import { TaskDrawer } from "../components/TaskDrawer";
import { ChatPanel } from "../components/chat/ChatPanel";
import { WorkflowsPanel } from "../components/workflows/WorkflowsPanel";
import { FilesPanel } from "../components/files/FilesPanel";
// Lazy for the same reason Monaco is: xterm only downloads for people who
// actually open the tab.
const TerminalPanel = lazy(() => import("../components/terminal/TerminalPanel"));
import { OrgRunView } from "../components/orgs/OrgRunView";
import { NARROW, useMediaQuery } from "../lib/useMediaQuery";
import { PreviewsPanel } from "../components/previews/PreviewsPanel";
import { ImportIssuesModal } from "../components/ImportIssuesModal";
import { ProjectSettings } from "../components/ProjectSettings";
import { BrainPanel } from "../components/BrainPanel";
import { StoragePanel } from "../components/StoragePanel";
import { PublishModal } from "../components/PublishModal";
import { Icon } from "../components/ui/Icon";
import { gradientFor } from "../components/ui/Surface";
import { springy, tappable } from "../lib/motion";

const TABS = [
  { key: "board", label: "Tasks Board" },
  { key: "workflows", label: "Workflows" },
  { key: "files", label: "Files" },
  { key: "terminal", label: "Terminal" },
  { key: "previews", label: "Previews" },
  { key: "brain", label: "Brain" },
  { key: "storage", label: "Storage" },
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
  const [showImport, setShowImport] = useState(false);
  const [settings, setSettings] = useState(false);
  const [publishing, setPublishing] = useState(false);
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

  // Fetched by id, not found in the list: the list filters to `kind='repo'`,
  // so scanning it left an app's own project resolving to null — a page with a
  // header reading "Project" and every setting silently defaulted.
  useEffect(() => {
    if (!projectId) return;
    api.project(projectId).then(setProject).catch(() => setProject(null));
  }, [projectId]);

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
      {!narrow && <ChatPanel projectId={projectId} workspaceId={project?.workspaceId} />}

      <div className="flex min-h-0 min-w-0 flex-col">
        <header className="border-b border-line bg-panel px-4 py-3 lg:px-6">
          {/* Two jobs, two sides. The left is identity — name on top, quiet
              facts underneath — and the right is the controls. One wrap-row of
              nine equal-weight chips read as clutter; a hierarchy reads at a
              glance. */}
          <div className="flex items-start gap-3">
            <motion.span
              initial={{ scale: 0.8, opacity: 0 }}
              animate={{ scale: 1, opacity: 1 }}
              transition={springy}
              className="mt-0.5 size-8 shrink-0 rounded-xl"
              style={{ background: gradientFor(project?.name ?? "Project") }}
            />
            <div className="min-w-0 flex-1">
              <div className="truncate text-base font-semibold leading-tight">
                {project?.name ?? "Project"}
              </div>
              <div className="mt-0.5 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-ink-dim">
                {project?.githubRepo && (
                  <a
                    href={`https://github.com/${project.githubRepo}`}
                    target="_blank"
                    rel="noreferrer"
                    title="Open this repository on GitHub"
                    className="truncate font-mono hover:text-ink"
                  >
                    {project.githubRepo}
                  </a>
                )}
                {project?.vcs === "none" && (
                  <span title={project.vcsNote ?? undefined} className="text-amber-700">
                    edits in place
                  </span>
                )}
                {/* Publishing turns a local-only project into one the whole
                    GitHub arc works on, so it stands where the repo name will. */}
                {project?.vcs === "git" && !project.githubRepo && (
                  <button
                    onClick={() => setPublishing(true)}
                    title="Create a GitHub repository for this project"
                    className="hover:text-accent"
                  >
                    publish to GitHub
                  </button>
                )}
                {project?.vcs === "git" && (
                  <GitSync projectId={project.id} onOpenFiles={() => setTab("files")} />
                )}
              </div>
            </div>
            <div className="flex shrink-0 items-center gap-2">
              {project && <AutonomyToggle project={project} onChanged={setProject} />}
              {project && (
                <button
                  onClick={() => setSettings(true)}
                  title="Project settings"
                  aria-label="Project settings"
                  className="ring-focus grid size-7 shrink-0 place-items-center rounded-lg text-ink-dim transition-colors hover:bg-panel-2 hover:text-ink"
                >
                  <Icon name="settings" size={15} />
                </button>
              )}
            </div>
          </div>

          <div className="mt-3 flex flex-nowrap items-center gap-3">
          <div className="flex min-w-0 flex-1 gap-1 overflow-x-auto rounded-xl bg-panel-2 p-1">
            {TABS.filter((t) => narrow || !("narrowOnly" in t)).map((t) => (
              <button
                key={t.key}
                onClick={() => setTab(t.key)}
                className={`ring-focus relative shrink-0 rounded-lg px-3 py-1.5 text-xs transition-colors ${
                  activeTab === t.key ? "text-ink" : "text-ink-dim hover:text-ink"
                }`}
              >
                {/* The white pill slides between tabs rather than blinking, so
                    the eye follows it to the new section instead of hunting
                    for where the highlight went. */}
                {activeTab === t.key && (
                  <motion.span
                    layoutId="project-tab"
                    transition={springy}
                    className="absolute inset-0 rounded-lg bg-panel shadow-sm"
                  />
                )}
                <span className={`relative ${activeTab === t.key ? "font-semibold" : ""}`}>
                  {t.label}
                </span>
              </button>
            ))}
          </div>
          {/* Only when the project actually is a GitHub repository — a button
              that could only refuse is worse than no button. */}
          {activeTab === "board" && project?.githubRepo && (
            <motion.button
              {...tappable}
              onClick={() => setShowImport(true)}
              className="ring-focus shrink-0 rounded-xl border border-line px-3 py-1.5 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
            >
              Import issues
            </motion.button>
          )}
          {activeTab === "board" && (
            <motion.button
              {...tappable}
              onClick={() => setShowNew(true)}
              className="ring-focus flex shrink-0 items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
            >
              <Icon name="plus" size={14} strokeWidth={2.5} />
              New task
            </motion.button>
          )}
          </div>
        </header>

        <div className="flex min-h-0 min-w-0 flex-1 flex-col">
          {/* mode="wait" so the leaving panel fades before the next enters —
              two boards cross-fading on top of each other is not a transition,
              it is a glitch. Kept to 150ms: this must never make the tabs feel
              slower than they were. */}
          <AnimatePresence mode="wait" initial={false}>
          <motion.div
            key={activeTab}
            initial={{ opacity: 0, y: 6 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -4 }}
            transition={{ duration: 0.15, ease: "easeOut" }}
            className="flex min-h-0 min-w-0 flex-1 flex-col"
          >
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
          {activeTab === "files" && (
            <FilesPanel projectId={projectId} tasks={tasks} />
          )}
          {activeTab === "terminal" && (
            <Suspense
              fallback={
                <div className="flex h-full items-center justify-center bg-[#1e1e1e] text-xs text-[#8c8c8c]">
                  Loading terminal…
                </div>
              }
            >
              <TerminalPanel projectId={projectId} />
            </Suspense>
          )}
          {activeTab === "previews" && <PreviewsPanel projectId={projectId} />}
          {activeTab === "brain" && <BrainPanel projectId={projectId} />}
          {activeTab === "storage" && <StoragePanel projectId={projectId} />}
          {activeTab === "chat" && (
            <ChatPanel projectId={projectId} workspaceId={project?.workspaceId} />
          )}
          </motion.div>
          </AnimatePresence>
        </div>
      </div>

      {/* Keyed because AnimatePresence tracks children by key, and these three
          conditional siblings appear and disappear independently — two can be
          on screen at once. Unkeyed, it has only child order to go on.

          The drawer's key is constant rather than the task id, so opening a
          different card is a prop change that keeps the panel's scroll and tab
          where they were, instead of tearing it down and sliding a new one in. */}
      <AnimatePresence>
        {settings && project && (
          <ProjectSettings
            key="project-settings"
            project={project}
            onChanged={setProject}
            onClose={() => setSettings(false)}
          />
        )}
        {publishing && project && (
          <PublishModal
            key="publish"
            projectId={project.id}
            onClose={() => setPublishing(false)}
            onDone={() => {
              setPublishing(false);
              api.project(project.id).then(setProject).catch(() => {});
            }}
          />
        )}
        {showImport && project && (
          <ImportIssuesModal
            key="import-issues"
            projectId={project.id}
            onClose={() => setShowImport(false)}
            onImported={() => {
              setShowImport(false);
              refresh();
            }}
          />
        )}
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
