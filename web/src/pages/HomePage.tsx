import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, Beat, PlanLimit, Project, Routine, Task } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { useActivity } from "../lib/activity";
import { KIND_LABEL, useInbox } from "../lib/inbox";
import { isWorking } from "../lib/runStatus";
import { soonestBurnout } from "../lib/forecast";
import { Stat } from "../components/Stat";
import { SpendBars } from "../components/spend/SpendBars";
import { isCurrent, resetIn, statusLabel, statusTone, windowLabel } from "../lib/usage";
import { Card, gradientFor, Item, Page, Stagger } from "../components/ui/Surface";
import { PageHeader } from "../components/ui/Layout";
import { buttonClasses } from "../components/ui/Button";
import { Badge, StatusDot } from "../components/ui/Badge";
import { ArrowRight, CalendarClock, ChevronRight, CheckCheck, Coins, FolderPlus, Hand, HeartPulse, Play, Plus } from "lucide-react";

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
  const [routines, setRoutines] = useState<Routine[]>([]);
  const [beats, setBeats] = useState<{ agent: string; beat: Beat }[]>([]);
  // The budget that runs out soonest at this rate, if any does before it resets.
  const [burnout, setBurnout] = useState<{ name: string; at: string } | null>(null);
  useEffect(() => {
    api
      .budgets()
      .then((r) => setBurnout(soonestBurnout(r.policies)))
      .catch(() => setBurnout(null));
  }, []);

  useEffect(() => {
    if (!active) return;
    api.projects(active.id).then((r) => setProjects(r.projects)).catch(() => {});
    api.tasks({ workspaceId: active.id }).then((r) => setTasks(r.tasks)).catch(() => {});
    api.usage().then((r) => setLimits(r.limits)).catch(() => {});
    api.routines(active.id).then((r) => setRoutines(r.routines)).catch(() => {});
    api.workspaceHeartbeats(active.id).then((r) => setBeats(r.beats)).catch(() => {});
  }, [active]);

  const live = activity?.live ?? [];
  const working = live.filter((r) => isWorking(r.status)).length;
  const queued = live.filter((r) => r.status === "queued").length;
  // Everything waiting on a person, not only the runs parked on one — the
  // inbox is the one list of that.
  const { items: inboxItems } = useInbox();
  const blocked = inboxItems ?? [];
  const review = tasks.filter((t) => t.boardColumn === "review").length;

  // What the last fortnight actually cost, rather than the sum of each card's
  // most recent run — which is what this showed before and is not a total of
  // anything a person would recognise.
  const today = activity?.spend.today ?? 0;
  const fortnight = activity?.spend.window ?? 0;
  const runs = (activity?.spend.daily ?? []).reduce((n, d) => n + d.runs, 0);

  const now = Date.now();
  const plan = limits.filter((l) => isCurrent(l.resetsAt, now));

  const overBudget = activity?.gate.state === "over_budget";

  return (
    <Page>
      <PageHeader
        title={`${greeting()}`}
        description={
          working || blocked.length
            ? `${working} run${working === 1 ? "" : "s"} working${blocked.length ? ` · ${blocked.length} waiting on you` : ""}.`
            : "Nothing running right now."
        }
        actions={
          <>
            <Link to="/projects?new=1" className={buttonClasses({ size: "sm" })}>
              <FolderPlus className="size-3.5" />
              New project
            </Link>
            <Link to="/activity" className={buttonClasses({ size: "sm", variant: "ghost" })}>
              Activity
              <ArrowRight className="size-3.5" />
            </Link>
          </>
        }
      />

      <Stagger className="grid grid-cols-[repeat(2,minmax(0,1fr))] gap-3 lg:grid-cols-[repeat(4,minmax(0,1fr))]">
        <Stat
          label="Working now"
          value={String(working)}
          icon={Play}
          tint="indigo"
          accent="var(--color-fg)"
          to="/activity"
          hint={queued ? `${queued} queued behind` : undefined}
        />
        {/* The most actionable number on the page. */}
        <Stat
          label="Waiting on you"
          value={String(blocked.length)}
          icon={Hand}
          tint={blocked.length ? "amber" : "slate"}
          accent={blocked.length ? "var(--color-warning-fg)" : "var(--color-fg)"}
          to="/inbox"
        />
        <Stat label="Ready to review" value={String(review)} icon={CheckCheck} tint="violet" accent="var(--color-fg)" />
        <Stat
          label={activity?.budgetUsd ? `Spent today of $${activity.budgetUsd.toFixed(0)}` : "Spent today"}
          value={`$${today.toFixed(2)}`}
          hint={
            overBudget
              ? `\u201c${activity!.gate.state === "over_budget" ? activity!.gate.policy : ""}\u201d is spent`
              : burnout
                ? `\u201c${burnout.name}\u201d runs out ~${new Date(burnout.at).toLocaleString(undefined, { weekday: "short", hour: "numeric" })}`
                : undefined
          }
          icon={Coins}
          tint={overBudget ? "rose" : "mint"}
          accent={overBudget ? "var(--color-danger-fg)" : "var(--color-fg)"}
          to="/activity"
        />
      </Stagger>

      <div className="mt-4 grid grid-cols-1 items-start gap-4 lg:grid-cols-[minmax(0,1.6fr)_minmax(0,1fr)]">
        <div className="flex min-w-0 flex-col gap-4">
          {/* Only when there is something to act on — a permanent "nothing is
              blocked" panel is a row of pixels that never changes. */}
          {blocked.length > 0 && (
            <Panel
              title={
                <span className="flex items-center gap-2">
                  <StatusDot tone="warning" pulse />
                  {blocked.length === 1 ? "One thing is waiting on you" : `${blocked.length} things are waiting on you`}
                </span>
              }
              action={<SoftLink to="/inbox">open the inbox</SoftLink>}
            >
              <ul className="divide-y divide-border">
                {blocked.slice(0, 5).map((b) => (
                  <li key={b.key} className="flex items-center gap-2.5 px-4 py-2 text-[13px]">
                    <Badge tone="warning">{KIND_LABEL[b.kind]}</Badge>
                    <Link to={b.link} className="min-w-0 flex-1 truncate hover:text-accent-fg">
                      {b.title}
                    </Link>
                  </li>
                ))}
              </ul>
            </Panel>
          )}

          <Panel title="Live runs" action={<SoftLink to="/activity">all activity</SoftLink>}>
            {live.length === 0 ? (
              <p className="px-4 py-6 text-center text-xs text-fg-muted">Nothing running. Start a card from a project's board.</p>
            ) : (
              <ul className="divide-y divide-border">
                {live.slice(0, 6).map((r) => (
                  <li key={r.id} className="flex items-center gap-2.5 px-4 py-2 text-[13px]">
                    <StatusDot tone={isWorking(r.status) ? "accent" : r.status === "queued" ? "neutral" : "warning"} pulse={isWorking(r.status)} />
                    <Link
                      to={r.projectId ? `/projects/${r.projectId}${r.taskId ? `?task=${r.taskId}` : ""}` : "/activity"}
                      className="min-w-0 flex-1 truncate hover:text-accent-fg"
                    >
                      {r.label}
                    </Link>
                    {r.projectName && <span className="hidden shrink-0 truncate text-xs text-fg-muted sm:inline">{r.projectName}</span>}
                    <span className="shrink-0 text-xs text-fg-subtle">{r.holdReason ? "held" : r.status.replace("_", " ")}</span>
                  </li>
                ))}
              </ul>
            )}
          </Panel>

          <div>
            <div className="mb-2 flex items-center justify-between">
              <h2 className="text-[13px] font-medium">Projects</h2>
              <SoftLink to="/projects">all projects</SoftLink>
            </div>
            <Stagger className="grid grid-cols-1 gap-2 sm:grid-cols-2">
              {projects.map((p) => {
                const mine = tasks.filter((t) => t.projectId === p.id);
                const busy = mine.filter((t) => t.boardColumn === "running").length;
                const waiting = mine.filter((t) => t.boardColumn === "review").length;
                return (
                  <Item key={p.id}>
                    <Card to={`/projects/${p.id}`} className="flex items-center gap-3 p-3">
                      <span className="size-8 shrink-0 rounded-md" style={{ background: gradientFor(p.name) }} aria-hidden />
                      <div className="min-w-0 flex-1">
                        <div className="truncate text-[13px] font-medium">{p.name}</div>
                        <div className="truncate font-mono text-[11px] text-fg-subtle">{p.githubRepo ?? p.path}</div>
                      </div>
                      <div className="flex shrink-0 flex-col items-end gap-1">
                        {busy > 0 && <Badge tone="accent">{busy} running</Badge>}
                        {waiting > 0 && <Badge tone="complex">{waiting} to review</Badge>}
                        {busy === 0 && waiting === 0 && (
                          <span className="text-[11px] text-fg-subtle">{mine.length ? `${mine.length} cards` : "no cards"}</span>
                        )}
                      </div>
                    </Card>
                  </Item>
                );
              })}
              <Item>
                <Link
                  to="/projects?new=1"
                  className="ring-focus flex h-full min-h-[58px] items-center justify-center gap-2 rounded-lg border border-dashed border-border text-[13px] text-fg-muted transition-colors hover:border-accent hover:text-accent-fg"
                >
                  <Plus className="size-4" /> Load a folder
                </Link>
              </Item>
            </Stagger>
          </div>
        </div>

        <div className="flex min-w-0 flex-col gap-4">
          <Panel title="Last 14 days" action={<SoftLink to="/activity">breakdown</SoftLink>}>
            <div className="px-4 pb-4 pt-1">
              <div className="flex items-baseline gap-2">
                <span className="tabular text-xl font-semibold tracking-tight">${fortnight.toFixed(2)}</span>
                <span className="text-xs text-fg-muted">
                  across {runs} run{runs === 1 ? "" : "s"}
                </span>
              </div>
              <div className="mt-3">
                <SpendBars daily={activity?.spend.daily ?? []} height={56} />
              </div>
            </div>
          </Panel>

          <Panel title="Plan limits" action={<SoftLink to="/activity">history</SoftLink>}>
            {plan.length === 0 ? (
              // Not an error, and not a zero: aichip learns this from the CLI as
              // it works, so before the first run there is genuinely nothing.
              <p className="px-4 pb-4 text-xs leading-relaxed text-fg-muted">
                Nothing heard yet — your CLI reports where your plan stands as it works, so this fills in after a run.
              </p>
            ) : (
              <ul className="space-y-2 px-4 pb-4">
                {plan.map((l) => {
                  const tone = statusTone(l.status);
                  const reset = resetIn(l.resetsAt, now);
                  return (
                    <li key={`${l.engine}-${l.limitType}`} className="flex items-baseline gap-2 text-xs">
                      <span className={`size-1.5 shrink-0 rounded-full ${tone.dot}`} />
                      <span className="font-medium">{windowLabel(l.limitType)}</span>
                      <span className={tone.text}>{statusLabel(l.status)}</span>
                      {reset && <span className="ml-auto shrink-0 text-fg-subtle">turns over {reset}</span>}
                    </li>
                  );
                })}
              </ul>
            )}
          </Panel>

          {/* Agents that pull their own work, and what they last picked up.
              Only once there is something to show. */}
          {beats.length > 0 && (
            <Panel title="Heartbeats" action={<SoftLink to="/org">org chart</SoftLink>}>
              <ul className="space-y-2 px-4 pb-4">
                {beats.slice(0, 5).map(({ agent, beat }, i) => (
                  <li key={i} className="flex items-center gap-2 text-xs">
                    <HeartPulse className="size-3.5 shrink-0 text-danger-fg" aria-hidden />
                    <span className="shrink-0 font-medium">{agent}</span>
                    <span className="min-w-0 truncate text-fg-muted">
                      {beat.outcome === "started" ? `picked up “${beat.taskTitle ?? "a card"}”` : beat.outcome === "fired" ? "ran its manager pass" : `held — ${beat.detail}`}
                    </span>
                    <span className="ml-auto shrink-0 text-fg-subtle">{new Date(beat.at).toLocaleTimeString([], { timeStyle: "short" })}</span>
                  </li>
                ))}
              </ul>
            </Panel>
          )}

          {/* Only for people who have routines — Home must not advertise
              features at someone who came to see their work. */}
          {routines.length > 0 && (
            <Panel title="Next routines" action={<SoftLink to="/routines">all routines</SoftLink>}>
              <ul className="space-y-2 px-4 pb-4">
                {routines
                  .filter((r) => r.enabled && r.nextAt)
                  .sort((a, b) => (a.nextAt! < b.nextAt! ? -1 : 1))
                  .slice(0, 4)
                  .map((r) => (
                    <li key={r.id} className="flex items-center gap-2 text-xs">
                      <CalendarClock className="size-3.5 shrink-0 text-fg-subtle" />
                      <span className="truncate font-medium">{r.name}</span>
                      {r.lastError && (
                        <Badge tone="danger" title={r.lastError}>
                          failed
                        </Badge>
                      )}
                      <span className="ml-auto shrink-0 text-fg-subtle">{nextIn(r.nextAt!)}</span>
                    </li>
                  ))}
                {routines.every((r) => !r.enabled) && <li className="text-xs text-fg-muted">All paused.</li>}
              </ul>
            </Panel>
          )}
        </div>
      </div>
    </Page>
  );
}

/** A titled box on the dashboard. */
function Panel({ title, action, children }: { title: React.ReactNode; action?: React.ReactNode; children: React.ReactNode }) {
  return (
    <section className="overflow-hidden rounded-lg border border-border bg-panel shadow-[var(--shadow-xs)]">
      <div className="flex h-10 items-center justify-between gap-2 px-4">
        <h2 className="text-[13px] font-medium">{title}</h2>
        {action}
      </div>
      {children}
    </section>
  );
}

function SoftLink({ to, children }: { to: string; children: React.ReactNode }) {
  return (
    <Link
      to={to}
      className="group inline-flex items-center gap-0.5 text-xs text-fg-muted transition-colors hover:text-fg"
    >
      {children}
      <ChevronRight className="size-3.5 transition-transform duration-200 group-hover:translate-x-0.5" />
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

/** "in 2 h" / "in 3 days" — the routine card's whole answer. */
function nextIn(iso: string): string {
  const mins = Math.max(0, Math.round((new Date(iso).getTime() - Date.now()) / 60000));
  if (mins < 1) return "about now";
  if (mins < 60) return `in ${mins} min`;
  if (mins < 60 * 48) return `in ${Math.round(mins / 60)} h`;
  return `in ${Math.round(mins / 1440)} days`;
}
