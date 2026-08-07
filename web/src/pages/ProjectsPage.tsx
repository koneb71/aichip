import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { api, Project } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { FolderBrowserModal } from "../components/FolderBrowserModal";
import { CloneRepoModal } from "../components/CloneRepoModal";

export default function ProjectsPage() {
  const { active } = useWorkspace();
  const [projects, setProjects] = useState<Project[]>([]);
  const [params, setParams] = useSearchParams();
  const showBrowser = params.get("new") === "1";
  const showClone = params.get("new") === "clone";
  const navigate = useNavigate();

  const refresh = useCallback(() => {
    if (!active) return;
    api.projects(active.id).then((r) => setProjects(r.projects)).catch(() => {});
  }, [active]);

  useEffect(refresh, [refresh]);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex items-center justify-between">
        <h1 className="text-xl font-bold tracking-tight">Projects</h1>
        <div className="flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setParams({ new: "clone" })}
            className="rounded-lg border border-line px-3 py-1.5 text-sm hover:border-ink-dim"
          >
            Clone from GitHub
          </motion.button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setParams({ new: "1" })}
            className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white"
          >
            + Load folder
          </motion.button>
        </div>
      </div>

      <div className="mt-6 grid max-w-4xl grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {projects.map((p) => (
          <Link key={p.id} to={`/projects/${p.id}`}>
            <motion.div
              layout
              whileHover={{ y: -2 }}
              className="card-shadow rounded-xl border border-line bg-panel p-4"
            >
              <div className="text-sm font-semibold">{p.name}</div>
              <div className="mt-1 truncate text-xs text-ink-dim">{p.path}</div>
              <div className="mt-3 text-[11px] text-ink-dim">
                {p.vcs === "git" ? (
                  <>base: {p.defaultBranch}</>
                ) : (
                  <span
                    title={p.vcsNote ?? undefined}
                    className="rounded-full bg-amber-50 px-2 py-0.5 text-amber-700"
                  >
                    no version control — edits in place
                  </span>
                )}
              </div>
            </motion.div>
          </Link>
        ))}
        {projects.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
            No projects yet — load a folder to get started.
          </div>
        )}
      </div>

      <AnimatePresence>
        {showClone && active && (
          <CloneRepoModal
            workspaceId={active.id}
            onClose={() => setParams({})}
            onCloned={(projectId: string) => {
              setParams({});
              refresh();
              navigate(`/projects/${projectId}`);
            }}
          />
        )}
        {showBrowser && active && (
          <FolderBrowserModal
            onClose={() => setParams({})}
            onPick={async (path) => {
              const added = await api.addProject(active.id, path);
              refresh();
              return { vcs: added.vcs, vcsNote: added.vcsNote };
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}
