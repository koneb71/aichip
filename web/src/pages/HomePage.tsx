import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { motion } from "framer-motion";
import { api, Project, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";

export default function HomePage() {
  const { active } = useWorkspace();
  const [projects, setProjects] = useState<Project[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);

  useEffect(() => {
    if (!active) return;
    api.projects(active.id).then((r) => setProjects(r.projects)).catch(() => {});
    api.tasks({ workspaceId: active.id }).then((r) => setTasks(r.tasks)).catch(() => {});
  }, [active]);

  const running = tasks.filter((t) => t.boardColumn === "running").length;
  const review = tasks.filter((t) => t.boardColumn === "review").length;
  const spent = tasks.reduce((sum, t) => sum + (t.costUsd ?? 0), 0);

  return (
    <div className="h-full overflow-y-auto p-5 sm:p-8">
      <motion.h1
        initial={{ opacity: 0, y: -6 }}
        animate={{ opacity: 1, y: 0 }}
        className="text-2xl font-bold tracking-tight"
      >
        {greeting()}
      </motion.h1>
      <p className="mt-1 text-sm text-ink-dim">
        {active ? `Workspace: ${active.name}` : "Loading workspace…"}
      </p>

      {/* `minmax(0,1fr)`, not `1fr`: three tiles whose labels set a min-content
          floor is enough to push the page wider than a phone, and `max-w-2xl`
          cannot claw that back — the whole page then scrolls sideways. */}
      <div className="mt-6 grid max-w-2xl grid-cols-[repeat(3,minmax(0,1fr))] gap-2 sm:gap-4">
        <Stat label="Agents running" value={String(running)} accent="var(--color-tier-medium)" />
        <Stat label="Awaiting review" value={String(review)} accent="var(--color-tier-complex)" />
        <Stat label="Session spend" value={`$${spent.toFixed(2)}`} accent="var(--color-tier-easy)" />
      </div>

      <h2 className="mt-10 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Projects
      </h2>
      <div className="mt-3 grid max-w-4xl grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {projects.map((p) => (
          <Link key={p.id} to={`/projects/${p.id}`}>
            <motion.div
              whileHover={{ y: -2 }}
              className="card-shadow rounded-xl border border-line bg-panel p-4"
            >
              <div className="text-sm font-semibold">{p.name}</div>
              <div className="mt-1 truncate text-xs text-ink-dim">{p.path}</div>
            </motion.div>
          </Link>
        ))}
        <Link to="/projects?new=1">
          <div className="flex h-full min-h-[76px] items-center justify-center rounded-xl border border-dashed border-line text-sm text-ink-dim hover:border-accent hover:text-accent">
            + Load a folder
          </div>
        </Link>
      </div>
    </div>
  );
}

function Stat({ label, value, accent }: { label: string; value: string; accent: string }) {
  return (
    <div className="card-shadow min-w-0 rounded-xl border border-line bg-panel p-3 sm:p-4">
      <div className="truncate text-xl font-bold sm:text-2xl" style={{ color: accent }}>
        {value}
      </div>
      <div className="mt-1 text-[11px] text-ink-dim sm:text-xs">{label}</div>
    </div>
  );
}

function greeting() {
  const h = new Date().getHours();
  if (h < 5) return "Working late";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}
