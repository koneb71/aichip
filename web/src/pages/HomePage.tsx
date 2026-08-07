import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { motion } from "framer-motion";
import { api, PlanLimit, Project, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { useActivity } from "../lib/activity";
import { isWorking } from "../lib/runStatus";
import { Stat } from "../components/Stat";
import { SpendBars } from "../components/spend/SpendBars";
import { isCurrent, resetIn, statusLabel, statusTone, windowLabel } from "../lib/usage";

/**
 * The page you land on: what is happening, what is waiting for you, and what it
 * has cost.
 *
 * It reads the same activity poll the Activity page does — the context is
 * mounted above the router, so this costs no extra request — and links into it
 * rather than restating it. Home is the glance; Activity is the detail.
 */
export default function HomePage() {
  const { active } = useWorkspace();
  const { activity } = useActivity();
  const [projects, setProjects] = useState<Project[]>([]);
  const [tasks, setTasks] = useState<Task[]>([]);
  const [limits, setLimits] = useState<PlanLimit[]>([]);

  useEffect(() => {
    if (!active) return;
    api.projects(active.id).then((r) => setProjects(r.projects)).catch(() => {});
    api.tasks({ workspaceId: active.id }).then((r) => setTasks(r.tasks)).catch(() => {});
    api.usage().then((r) => setLimits(r.limits)).catch(() => {});
  }, [active]);

  const live = activity?.live ?? [];
  const working = live.filter((r) => isWorking(r.status)).length;
  const queued = live.filter((r) => r.status === "queued").length;
  const blocked = activity?.blocked ?? [];
  const review = tasks.filter((t) => t.boardColumn === "review").length;

  // What the last fortnight actually cost, rather than the sum of each card's
  // most recent run — which is what this showed before and is not a total of
  // anything a person would recognise.
  const today = activity?.spend.today ?? 0;
  const fortnight = activity?.spend.window ?? 0;
  const runs = (activity?.spend.daily ?? []).reduce((n, d) => n + d.runs, 0);

  const now = Date.now();
  const plan = limits.filter((l) => isCurrent(l.resetsAt, now));

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

      {/* `minmax(0,1fr)`, not `1fr`: tiles whose labels set a min-content floor
          are enough to push the page wider than a phone, and `max-w-*` cannot
          claw that back — the whole page then scrolls sideways. */}
      <div className="mt-6 grid max-w-4xl grid-cols-[repeat(2,minmax(0,1fr))] gap-2 sm:grid-cols-[repeat(4,minmax(0,1fr))] sm:gap-4">
        <Stat
          label="Working now"
          value={String(working)}
          accent="var(--color-tier-medium)"
          to="/activity"
          hint={queued ? `${queued} queued behind` : undefined}
        />
        {/* The most actionable number on the page, and the one it did not have. */}
        <Stat
          label="Waiting on you"
          value={String(blocked.length)}
          accent={blocked.length ? "#d97706" : "var(--color-ink-dim)"}
          to="/activity"
        />
        <Stat
          label="Ready to review"
          value={String(review)}
          accent="var(--color-tier-complex)"
        />
        <Stat
          label={activity?.budgetUsd ? `Spent today of $${activity.budgetUsd.toFixed(0)}` : "Spent today"}
          value={`$${today.toFixed(2)}`}
          accent={activity?.gate.state === "over_budget" ? "#d97706" : "var(--color-tier-easy)"}
          to="/activity"
        />
      </div>

      {/* Only when there is something to act on. A permanent empty panel
          saying "nothing is blocked" is a row of pixels that never changes. */}
      {blocked.length > 0 && (
        <motion.div
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-4 max-w-4xl rounded-xl border border-amber-200 bg-amber-50 p-4"
        >
          <div className="text-xs font-semibold text-amber-900">
            {blocked.length === 1 ? "A run is waiting for you" : `${blocked.length} runs are waiting for you`}
          </div>
          <ul className="mt-2 space-y-1">
            {/* Keyed with the index too: one run can be holding a permission
                prompt and a plan at once, so `runId` alone is not unique. */}
            {blocked.slice(0, 4).map((b, i) => (
              <li key={`${b.runId}-${i}`} className="truncate text-xs text-amber-900/90">
                <span className="mr-1.5 rounded bg-amber-200/60 px-1.5 py-0.5 text-[10px] uppercase tracking-wide">
                  {b.kind === "plan" ? "plan" : "permission"}
                </span>
                {b.label}
              </li>
            ))}
          </ul>
          <Link to="/activity" className="mt-2 inline-block text-xs font-medium text-amber-900 underline">
            Go and answer {blocked.length === 1 ? "it" : "them"} →
          </Link>
        </motion.div>
      )}

      {/* `items-start`: the plan card is a short list and the spend card is a
          chart, so stretching them to a shared height leaves a tall empty
          box next to a full one. */}
      <div className="mt-4 grid max-w-4xl grid-cols-1 items-start gap-4 lg:grid-cols-2">
        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <div className="flex items-baseline justify-between gap-2">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-ink-dim">
              Last 14 days
            </h2>
            <Link to="/activity" className="text-[11px] text-ink-dim hover:text-ink">
              breakdown →
            </Link>
          </div>
          <div className="mt-1 flex items-baseline gap-2">
            <span className="text-xl font-bold">${fortnight.toFixed(2)}</span>
            <span className="text-[11px] text-ink-dim">
              across {runs} run{runs === 1 ? "" : "s"}
            </span>
          </div>
          <div className="mt-3">
            <SpendBars daily={activity?.spend.daily ?? []} height={64} />
          </div>
        </div>

        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <div className="flex items-baseline justify-between gap-2">
            <h2 className="text-xs font-semibold uppercase tracking-wider text-ink-dim">
              Your Claude plan
            </h2>
            <Link to="/activity" className="text-[11px] text-ink-dim hover:text-ink">
              history →
            </Link>
          </div>
          {plan.length === 0 ? (
            // Not an error, and not a zero: aichip learns this from the CLI as
            // it works, so before the first run there is genuinely nothing.
            <p className="mt-2 text-xs text-ink-dim">
              Nothing heard yet — your CLI reports where your plan stands as it
              works, so this fills in after a run.
            </p>
          ) : (
            <ul className="mt-2 space-y-2">
              {plan.map((l) => {
                const tone = statusTone(l.status);
                const reset = resetIn(l.resetsAt, now);
                return (
                  <li key={`${l.engine}-${l.limitType}`} className="flex items-baseline gap-2 text-xs">
                    <span className={`size-1.5 shrink-0 rounded-full ${tone.dot}`} />
                    <span className="font-medium">{windowLabel(l.limitType)}</span>
                    <span className={tone.text}>{statusLabel(l.status)}</span>
                    {reset && <span className="ml-auto shrink-0 text-ink-dim">turns over {reset}</span>}
                  </li>
                );
              })}
            </ul>
          )}
        </div>
      </div>

      <h2 className="mt-10 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Projects
      </h2>
      <div className="mt-3 grid max-w-4xl grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
        {projects.map((p) => {
          const mine = tasks.filter((t) => t.projectId === p.id);
          const busy = mine.filter((t) => t.boardColumn === "running").length;
          const waiting = mine.filter((t) => t.boardColumn === "review").length;
          return (
            <Link key={p.id} to={`/projects/${p.id}`}>
              <motion.div
                whileHover={{ y: -2 }}
                className="card-shadow h-full rounded-xl border border-line bg-panel p-4"
              >
                {/* Wrapping, not shrinking: `owner/repo` is long and the
                    project's own name is what you are scanning for, so the
                    chip drops to its own line rather than truncating the
                    name to "resu…". */}
                <div className="flex flex-wrap items-baseline gap-x-2">
                  <span className="max-w-full truncate text-sm font-semibold">{p.name}</span>
                  {p.githubRepo && (
                    <span className="max-w-full truncate font-mono text-[10px] text-ink-dim">
                      {p.githubRepo}
                    </span>
                  )}
                </div>
                <div className="mt-1 truncate text-xs text-ink-dim">{p.path}</div>
                <div className="mt-2 flex flex-wrap items-center gap-1.5 text-[11px]">
                  {busy > 0 && (
                    <span className="rounded-full bg-tier-medium-soft px-2 py-0.5 text-tier-medium">
                      {busy} running
                    </span>
                  )}
                  {waiting > 0 && (
                    <span className="rounded-full bg-tier-complex-soft px-2 py-0.5 text-tier-complex">
                      {waiting} to review
                    </span>
                  )}
                  {busy === 0 && waiting === 0 && (
                    <span className="text-ink-dim">
                      {mine.length ? `${mine.length} card${mine.length === 1 ? "" : "s"}` : "no cards yet"}
                    </span>
                  )}
                </div>
              </motion.div>
            </Link>
          );
        })}
        <Link to="/projects?new=1">
          <div className="flex h-full min-h-[76px] items-center justify-center rounded-xl border border-dashed border-line text-sm text-ink-dim hover:border-accent hover:text-accent">
            + Load a folder
          </div>
        </Link>
      </div>
    </div>
  );
}

function greeting() {
  const h = new Date().getHours();
  if (h < 5) return "Still up?";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}
