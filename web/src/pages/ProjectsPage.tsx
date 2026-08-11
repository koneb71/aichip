import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useSearchParams } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { api, Project } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { FolderBrowserModal } from "../components/FolderBrowserModal";
import { CloneRepoModal } from "../components/CloneRepoModal";
import { Card, Empty, gradientFor, Item, Page, PageHead, Stagger } from "../components/ui/Surface";
import { Icon } from "../components/ui/Icon";
import { tappable } from "../lib/motion";

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
    <Page>
      <PageHead
        title="Projects"
        subtitle="Every folder aichip can work in. A card runs in its own git worktree, so nothing an agent does reaches your checkout until you land it."
        actions={
          <>
            <motion.button
              {...tappable}
              onClick={() => setParams({ new: "clone" })}
              className="ring-focus rounded-xl border border-line bg-panel px-3.5 py-2 text-sm font-medium transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
            >
              Clone from GitHub
            </motion.button>
            <motion.button
              {...tappable}
              onClick={() => setParams({ new: "1" })}
              className="ring-focus flex items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
            >
              <Icon name="plus" size={15} strokeWidth={2.5} />
              Load folder
            </motion.button>
          </>
        }
      />

      <Stagger className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {projects.map((p) => (
          <Item key={p.id}>
            <Card to={`/projects/${p.id}`} className="h-full overflow-hidden">
              <div
                className="sheen relative h-14 w-full overflow-hidden"
                style={{ background: gradientFor(p.name) }}
              >
                <div className="absolute inset-0 bg-gradient-to-t from-black/15 to-transparent" />
              </div>
              <div className="p-4">
                <div className="truncate text-sm font-semibold">{p.name}</div>
                <div className="mt-1 truncate text-xs text-ink-dim">{p.path}</div>
                <div className="mt-2.5 text-[11px] text-ink-dim">
                  {p.vcs === "git" ? (
                    <span className="inline-flex items-center gap-1 rounded-full bg-panel-2 px-2 py-0.5">
                      base: {p.defaultBranch}
                    </span>
                  ) : (
                    <span
                      title={p.vcsNote ?? undefined}
                      className="rounded-full bg-amber-50 px-2 py-0.5 text-amber-700"
                    >
                      no version control — edits in place
                    </span>
                  )}
                </div>
              </div>
            </Card>
          </Item>
        ))}
        {projects.length === 0 && (
          <div className="col-span-full">
            <Empty
              icon={<Icon name="folder" size={28} />}
              title="No projects yet"
              hint="Load a folder from this machine, or clone one from GitHub, and aichip will start working in it."
            />
          </div>
        )}
      </Stagger>

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
    </Page>
  );
}
