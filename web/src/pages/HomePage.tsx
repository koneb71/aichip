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
import { Icon, IconName } from "../components/ui/Icon";
import { Card, gradientFor, Item, SectionLabel, Stagger, Tint, TintIcon } from "../components/ui/Surface";
import { itemVariants, listVariants, tappable, tileVariants } from "../lib/motion";

/**
 * The page you land on: what is happening, what is waiting for you, and what it
 * has cost.
 *
 * It reads the same activity poll the Activity page does — the context is
 * mounted above the router, so this costs no extra request — and links into it
 * rather than restating it. Home is the glance; Activity is the detail.
 *
 * The hero at the top is deliberately the largest thing on the page. Landing on
 * a wall of numbers tells you how the last fortnight went; landing on one line
 * and a search field tells you what to do next, which is what you actually came
 * for on nine visits out of ten.
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
    <div className="h-full overflow-y-auto">
      <Hero name={active?.name} />

      <div className="mx-auto max-w-5xl px-5 pb-16 sm:px-8">
        <Stagger className="grid grid-cols-[repeat(2,minmax(0,1fr))] gap-3 sm:grid-cols-[repeat(4,minmax(0,1fr))]">
          <Stat
            label="Working now"
            value={String(working)}
            icon="play"
            tint="indigo"
            accent="var(--color-tier-medium)"
            to="/activity"
            hint={queued ? `${queued} queued behind` : undefined}
          />
          {/* The most actionable number on the page, and the one it did not have. */}
          <Stat
            label="Waiting on you"
            value={String(blocked.length)}
            icon="bell"
            tint={blocked.length ? "amber" : "slate"}
            accent={blocked.length ? "#d97706" : "var(--color-ink-dim)"}
            to="/activity"
          />
          <Stat
            label="Ready to review"
            value={String(review)}
            icon="check"
            tint="violet"
            accent="var(--color-tier-complex)"
          />
          <Stat
            label={
              activity?.budgetUsd ? `Spent today of $${activity.budgetUsd.toFixed(0)}` : "Spent today"
            }
            value={`$${today.toFixed(2)}`}
            icon="coin"
            tint={activity?.gate.state === "over_budget" ? "amber" : "mint"}
            accent={activity?.gate.state === "over_budget" ? "#d97706" : "var(--color-tier-easy)"}
            to="/activity"
          />
        </Stagger>

        {/* Only when there is something to act on. A permanent empty panel
            saying "nothing is blocked" is a row of pixels that never changes. */}
        {blocked.length > 0 && (
          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            className="mt-5 overflow-hidden rounded-2xl border border-amber-200/70 bg-gradient-to-br from-amber-50 to-amber-50/40 p-4"
          >
            <div className="flex items-center gap-2.5">
              <TintIcon tint="amber" size={32}>
                <Icon name="bell" size={16} />
              </TintIcon>
              <div className="text-sm font-semibold text-amber-900">
                {blocked.length === 1
                  ? "A run is waiting for you"
                  : `${blocked.length} runs are waiting for you`}
              </div>
            </div>
            <ul className="mt-3 space-y-1.5">
              {/* Keyed with the index too: one run can be holding a permission
                  prompt and a plan at once, so `runId` alone is not unique. */}
              {blocked.slice(0, 4).map((b, i) => (
                <motion.li
                  key={`${b.runId}-${i}`}
                  initial={{ opacity: 0, x: -6 }}
                  animate={{ opacity: 1, x: 0 }}
                  transition={{ delay: 0.05 * i }}
                  className="flex items-center gap-2 truncate text-xs text-amber-900/90"
                >
                  <span className="shrink-0 rounded-md bg-amber-200/70 px-1.5 py-0.5 text-[10px] font-medium uppercase tracking-wide">
                    {b.kind === "plan" ? "plan" : "permission"}
                  </span>
                  <span className="truncate">{b.label}</span>
                </motion.li>
              ))}
            </ul>
            <Link
              to="/activity"
              className="group mt-3 inline-flex items-center gap-1 text-xs font-semibold text-amber-900"
            >
              Go and answer {blocked.length === 1 ? "it" : "them"}
              <span className="transition-transform duration-200 group-hover:translate-x-0.5">
                <Icon name="chevronRight" size={13} />
              </span>
            </Link>
          </motion.div>
        )}

        {/* `items-start`: the plan card is a short list and the spend card is a
            chart, so stretching them to a shared height leaves a tall empty
            box next to a full one. */}
        <Stagger className="mt-5 grid grid-cols-1 items-start gap-4 lg:grid-cols-2">
          <Item>
            <Card className="p-5">
              <div className="flex items-baseline justify-between gap-2">
                <h2 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-ink-dim">
                  Last 14 days
                </h2>
                <SoftLink to="/activity">breakdown</SoftLink>
              </div>
              <div className="mt-1.5 flex items-baseline gap-2">
                <span className="text-2xl font-bold tracking-tight">${fortnight.toFixed(2)}</span>
                <span className="text-[11px] text-ink-dim">
                  across {runs} run{runs === 1 ? "" : "s"}
                </span>
              </div>
              <div className="mt-4">
                <SpendBars daily={activity?.spend.daily ?? []} height={64} />
              </div>
            </Card>
          </Item>

          <Item>
            <Card className="p-5">
              <div className="flex items-baseline justify-between gap-2">
                <h2 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-ink-dim">
                  Your Claude plan
                </h2>
                <SoftLink to="/activity">history</SoftLink>
              </div>
              {plan.length === 0 ? (
                // Not an error, and not a zero: aichip learns this from the CLI
                // as it works, so before the first run there is genuinely nothing.
                <p className="mt-3 text-xs leading-relaxed text-ink-dim">
                  Nothing heard yet — your CLI reports where your plan stands as it works, so this
                  fills in after a run.
                </p>
              ) : (
                <ul className="mt-3 space-y-2.5">
                  {plan.map((l) => {
                    const tone = statusTone(l.status);
                    const reset = resetIn(l.resetsAt, now);
                    return (
                      <li
                        key={`${l.engine}-${l.limitType}`}
                        className="flex items-baseline gap-2 text-xs"
                      >
                        <span className={`size-1.5 shrink-0 rounded-full ${tone.dot}`} />
                        <span className="font-medium">{windowLabel(l.limitType)}</span>
                        <span className={tone.text}>{statusLabel(l.status)}</span>
                        {reset && (
                          <span className="ml-auto shrink-0 text-ink-dim">turns over {reset}</span>
                        )}
                      </li>
                    );
                  })}
                </ul>
              )}
            </Card>
          </Item>
        </Stagger>

        <div className="mt-10">
          <SectionLabel action={<SoftLink to="/projects">all projects</SoftLink>}>
            Projects
          </SectionLabel>
          <Stagger className="grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-3">
            {projects.map((p) => {
              const mine = tasks.filter((t) => t.projectId === p.id);
              const busy = mine.filter((t) => t.boardColumn === "running").length;
              const waiting = mine.filter((t) => t.boardColumn === "review").length;
              return (
                <Item key={p.id}>
                  <Card to={`/projects/${p.id}`} className="h-full overflow-hidden">
                    {/* A colour band rather than a thumbnail: there is no image
                        to show, but a project you have opened fifty times is
                        found by its colour long before you read its name. */}
                    <div
                      className="sheen relative h-16 w-full overflow-hidden"
                      style={{ background: gradientFor(p.name) }}
                    >
                      <div className="absolute inset-0 bg-gradient-to-t from-black/15 to-transparent" />
                    </div>
                    <div className="p-4">
                      {/* Wrapping, not shrinking: `owner/repo` is long and the
                          project's own name is what you are scanning for, so
                          the chip drops to its own line rather than truncating
                          the name to "resu…". */}
                      <div className="flex flex-wrap items-baseline gap-x-2">
                        <span className="max-w-full truncate text-sm font-semibold">{p.name}</span>
                        {p.githubRepo && (
                          <span className="max-w-full truncate font-mono text-[10px] text-ink-dim">
                            {p.githubRepo}
                          </span>
                        )}
                      </div>
                      <div className="mt-1 truncate text-xs text-ink-dim">{p.path}</div>
                      <div className="mt-2.5 flex flex-wrap items-center gap-1.5 text-[11px]">
                        {busy > 0 && (
                          <span className="rounded-full bg-tier-medium-soft px-2 py-0.5 font-medium text-tier-medium">
                            {busy} running
                          </span>
                        )}
                        {waiting > 0 && (
                          <span className="rounded-full bg-tier-complex-soft px-2 py-0.5 font-medium text-tier-complex">
                            {waiting} to review
                          </span>
                        )}
                        {busy === 0 && waiting === 0 && (
                          <span className="text-ink-dim">
                            {mine.length
                              ? `${mine.length} card${mine.length === 1 ? "" : "s"}`
                              : "no cards yet"}
                          </span>
                        )}
                      </div>
                    </div>
                  </Card>
                </Item>
              );
            })}
            <Item>
              <Link
                to="/projects?new=1"
                className="ring-focus group flex h-full min-h-[150px] flex-col items-center justify-center gap-2 rounded-2xl border border-dashed border-line text-sm text-ink-dim transition-colors hover:border-accent hover:bg-accent/[0.03] hover:text-accent"
              >
                <span className="grid size-10 place-items-center rounded-xl bg-panel-2 transition-transform duration-300 group-hover:scale-110 group-hover:bg-accent/10">
                  <Icon name="plus" size={18} />
                </span>
                Load a folder
              </Link>
            </Item>
          </Stagger>
        </div>
      </div>
    </div>
  );
}

/** Quick ways in. Each is one hue, so the row reads as a palette rather than a
 *  toolbar of identical grey buttons. */
const TOOLS: { to: string; label: string; icon: IconName; tint: Tint }[] = [
  { to: "/projects", label: "Projects", icon: "projects", tint: "indigo" },
  { to: "/activity", label: "Activity", icon: "activity", tint: "sky" },
  { to: "/agents", label: "Agents", icon: "agents", tint: "violet" },
  { to: "/skills", label: "Skills", icon: "skills", tint: "amber" },
  { to: "/teams", label: "Teams", icon: "teams", tint: "mint" },
  { to: "/apps", label: "Apps", icon: "apps", tint: "rose" },
  { to: "/knowledge", label: "Knowledge", icon: "knowledge", tint: "slate" },
];

function Hero({ name }: { name?: string }) {
  return (
    <div className="relative overflow-hidden">
      {/* Two drifting blobs behind a heavy blur. Pointer-events off and
          aria-hidden: it is a wash, not a thing on the page. */}
      <div aria-hidden className="pointer-events-none absolute inset-0 overflow-hidden">
        <div
          className="aurora absolute -top-40 left-1/2 size-[38rem] -translate-x-1/2 rounded-full opacity-[0.13] blur-3xl"
          style={{ background: "radial-gradient(circle, #6366f1, transparent 70%)" }}
        />
        <div
          className="aurora absolute -top-24 left-[68%] size-[30rem] rounded-full opacity-[0.11] blur-3xl"
          style={{ background: "radial-gradient(circle, #d946ef, transparent 70%)", animationDelay: "-6s" }}
        />
      </div>

      <div className="relative mx-auto max-w-3xl px-5 pb-10 pt-16 text-center sm:px-8 sm:pt-20">
        <motion.h1
          initial={{ opacity: 0, y: 14, filter: "blur(6px)" }}
          animate={{ opacity: 1, y: 0, filter: "blur(0px)" }}
          transition={{ duration: 0.5, ease: [0.22, 1, 0.36, 1] }}
          className="text-[34px] font-bold leading-tight tracking-tight sm:text-[42px]"
        >
          {greeting()}
        </motion.h1>
        <motion.p
          initial={{ opacity: 0, y: 8 }}
          animate={{ opacity: 1, y: 0 }}
          transition={{ delay: 0.08, duration: 0.4, ease: [0.22, 1, 0.36, 1] }}
          className="mt-2 text-sm text-ink-dim"
        >
          {name ? `Workspace: ${name}` : "Loading workspace…"}
        </motion.p>

        <HeroSearch />

        <motion.div
          variants={listVariants}
          initial="hidden"
          animate="show"
          className="mt-9 flex flex-wrap items-start justify-center gap-x-3 gap-y-4 sm:gap-x-6"
        >
          {TOOLS.map((t) => (
            <motion.div key={t.to} variants={tileVariants}>
              <Link
                to={t.to}
                className="ring-focus group flex w-[74px] flex-col items-center gap-2 rounded-xl py-1"
              >
                <motion.span
                  whileHover={{ y: -3, scale: 1.06 }}
                  whileTap={{ scale: 0.95 }}
                  transition={{ type: "spring", stiffness: 400, damping: 22 }}
                  className="block"
                >
                  <TintIcon tint={t.tint} size={52} className="shadow-[0_2px_10px_-4px_rgba(16,17,20,0.2)]">
                    <Icon name={t.icon} size={22} />
                  </TintIcon>
                </motion.span>
                <span className="text-[11px] font-medium text-ink-dim transition-colors group-hover:text-ink">
                  {t.label}
                </span>
              </Link>
            </motion.div>
          ))}
        </motion.div>
      </div>
    </div>
  );
}

/**
 * The hero's search field.
 *
 * Not a second search implementation — clicking it fires the same ⌘K the
 * palette in the sidebar already listens for, so there is one index, one set of
 * results, and one place to fix a bug in them.
 */
function HeroSearch() {
  const open = () =>
    window.dispatchEvent(new KeyboardEvent("keydown", { key: "k", metaKey: true, bubbles: true }));

  return (
    <motion.button
      {...tappable}
      onClick={open}
      initial={{ opacity: 0, y: 12 }}
      animate={{ opacity: 1, y: 0 }}
      transition={{ delay: 0.14, duration: 0.45, ease: [0.22, 1, 0.36, 1] }}
      className="ring-focus group mx-auto mt-7 flex w-full max-w-xl items-center gap-3 rounded-2xl border border-line bg-panel px-4 py-3.5 text-left card-shadow-md transition-[border-color,box-shadow] hover:border-accent/40 hover:card-shadow-lg"
    >
      <span className="text-ink-dim transition-colors group-hover:text-accent">
        <Icon name="search" size={18} />
      </span>
      <span className="flex-1 text-sm text-ink-dim">Search projects, cards, agents…</span>
      <kbd className="hidden rounded-md border border-line bg-panel-2 px-1.5 py-0.5 font-sans text-[10px] font-medium text-ink-dim sm:block">
        ⌘K
      </kbd>
    </motion.button>
  );
}

/** A quiet link with an arrow that nudges on hover. */
function SoftLink({ to, children }: { to: string; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="group inline-flex items-center gap-0.5 text-[11px] text-ink-dim transition-colors hover:text-ink"
    >
      {children}
      <span className="transition-transform duration-200 group-hover:translate-x-0.5">
        <Icon name="chevronRight" size={12} />
      </span>
    </Link>
  );
}

function greeting() {
  const h = new Date().getHours();
  if (h < 5) return "Still up?";
  if (h < 12) return "Good morning";
  if (h < 18) return "Good afternoon";
  return "Good evening";
}
