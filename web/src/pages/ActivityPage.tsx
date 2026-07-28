import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { ActivityRun, api } from "../lib/api";
import { useActivity, notificationsOn, toggleNotifications } from "../lib/activity";
import { useWorkspace } from "../lib/workspace";
import { isWorking, statusColor, statusLabel } from "../lib/runStatus";
import { OrgRunView } from "../components/orgs/OrgRunView";
import { PermissionRow } from "../components/PermissionRow";

/**
 * The operations view.
 *
 * Everything else in the app is organised by *where* work lives — this
 * project, that team. This page is organised by what needs attention now:
 * what is blocked on you first, then what is running, then what it has all
 * cost. It exists because the binding constraint on this product is a
 * rolling rate limit you cannot see.
 */
export default function ActivityPage() {
  const { active } = useWorkspace();
  const { activity: data, refresh: load } = useActivity();
  const [orgRun, setOrgRun] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  async function togglePause() {
    if (!data) return;
    setBusy(true);
    try {
      await api.pauseQueue(!data.paused);
      load();
    } finally {
      setBusy(false);
    }
  }

  const live = data?.live ?? [];
  const working = live.filter((r) => isWorking(r.status));
  const queued = live.filter((r) => r.status === "queued");
  const blocked = data?.blocked ?? [];

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex flex-wrap items-start justify-between gap-4">
        <div>
          <h1 className="text-2xl font-bold tracking-tight">Activity</h1>
          <p className="mt-1 text-sm text-ink-dim">
            Everything running across {active?.name ?? "this workspace"}.
          </p>
        </div>
        <div className="flex items-center gap-2">
          <NotifyToggle />
          <button
            onClick={togglePause}
            disabled={busy || !data}
            className={`rounded-lg border px-3 py-1.5 text-sm font-medium transition-colors disabled:opacity-50 ${
              data?.paused
                ? "border-amber-300 bg-amber-50 text-amber-800 hover:bg-amber-100"
                : "border-line bg-panel hover:bg-panel-2"
            }`}
          >
            {data?.paused ? "▶ Resume queue" : "❚❚ Pause queue"}
          </button>
        </div>
      </div>

      <AnimatePresence>
        {data && data.gate.state !== "open" && (
          <motion.div
            initial={{ opacity: 0, height: 0 }}
            animate={{ opacity: 1, height: "auto" }}
            exit={{ opacity: 0, height: 0 }}
            className="mt-4 overflow-hidden"
          >
            <div className="rounded-xl border border-amber-300 bg-amber-50 px-4 py-3 text-sm text-amber-900">
              {data.gate.state === "paused" ? (
                <>
                  <span className="font-semibold">Queue paused.</span> Nothing new
                  will start. Work already in flight keeps going — cancel a run to
                  stop it.
                </>
              ) : (
                <>
                  <span className="font-semibold">
                    Daily budget reached — ${data.gate.spentToday.toFixed(2)} of $
                    {data.gate.capUsd.toFixed(2)}.
                  </span>{" "}
                  Nothing new will start until midnight. Raise the cap below to
                  carry on today.
                </>
              )}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="mt-6 grid max-w-3xl grid-cols-2 gap-4 sm:grid-cols-4">
        <Stat label="Working now" value={String(working.length)} accent="var(--color-tier-medium)" />
        <Stat label="Waiting on you" value={String(blocked.length)} accent="#d97706" />
        <Stat label="Queued" value={String(queued.length)} accent="var(--color-ink-dim)" />
        <Stat
          label={
            data?.budgetUsd ? `Spent today of $${data.budgetUsd.toFixed(0)}` : "Spent today"
          }
          value={`$${(data?.spend.today ?? 0).toFixed(2)}`}
          accent={
            data?.gate.state === "over_budget"
              ? "#d97706"
              : "var(--color-tier-easy)"
          }
        />
      </div>

      {data?.budgetUsd != null && (
        <div className="mt-3 max-w-3xl">
          <div className="h-1.5 overflow-hidden rounded-full bg-panel-2">
            <div
              style={{
                width: `${Math.min(100, ((data.spend.today ?? 0) / data.budgetUsd) * 100)}%`,
              }}
              className={`h-full rounded-full transition-[width] duration-500 ${
                data.gate.state === "over_budget" ? "bg-amber-500" : "bg-tier-easy"
              }`}
            />
          </div>
        </div>
      )}

      {blocked.length > 0 && (
        <Section title="Waiting on you">
          <div className="flex flex-col gap-2">
            {blocked.map((b, i) =>
              b.kind === "permission" && b.requestId ? (
                <PermissionRow
                  key={b.requestId}
                  toolName={b.tool ?? "a tool"}
                  input={b.input}
                  context={b.label}
                  onAnswer={async (allowed) => {
                    await api.resolvePermission(b.requestId!, allowed);
                    load();
                  }}
                />
              ) : (
                <motion.button
                  key={`${b.runId}-plan-${i}`}
                  initial={{ opacity: 0, x: -6 }}
                  animate={{ opacity: 1, x: 0 }}
                  onClick={() => setOrgRun(b.runId)}
                  className="card-shadow flex items-center gap-3 rounded-xl border border-amber-300 bg-amber-50/60 px-4 py-3 text-left hover:bg-amber-50"
                >
                  <span className="text-lg">◧</span>
                  <div className="min-w-0">
                    <div className="truncate text-sm font-semibold">{b.label}</div>
                    <div className="truncate text-xs text-ink-dim">
                      A plan is ready for your review
                    </div>
                  </div>
                </motion.button>
              ),
            )}
          </div>
        </Section>
      )}

      <Section title={`Live runs${live.length ? ` (${live.length})` : ""}`}>
        {live.length === 0 ? (
          <div className="rounded-xl border border-dashed border-line px-4 py-8 text-center text-sm text-ink-dim">
            Nothing is running.
          </div>
        ) : (
          <div className="flex flex-col gap-2">
            <AnimatePresence initial={false}>
              {live.map((run) => (
                <RunRow key={run.id} run={run} onOpenOrg={() => setOrgRun(run.id)} />
              ))}
            </AnimatePresence>
          </div>
        )}
      </Section>

      <Section title="Spend, last 14 days">
        <div className="card-shadow rounded-xl border border-line bg-panel p-5">
          <div className="flex flex-wrap items-baseline justify-between gap-2">
            <div className="flex items-baseline gap-2">
              <span className="text-2xl font-bold">
                ${(data?.spend.window ?? 0).toFixed(2)}
              </span>
              <span className="text-xs text-ink-dim">
                across {(data?.spend.daily ?? []).reduce((n, d) => n + d.runs, 0)} runs
              </span>
            </div>
            <BudgetControl current={data?.budgetUsd ?? null} onSaved={load} />
          </div>
          <Bars daily={data?.spend.daily ?? []} />
        </div>

        {(data?.spend.byAgent.length ?? 0) > 0 && (
          <div className="card-shadow mt-4 rounded-xl border border-line bg-panel p-5">
            <div className="text-xs font-semibold uppercase tracking-wider text-ink-dim">
              By agent
            </div>
            <p className="mt-1 text-[11px] text-ink-dim/80">
              A run's cost is split evenly across its assignments — close, not exact.
            </p>
            <div className="mt-3 flex flex-col gap-2">
              {data!.spend.byAgent.map((a) => {
                const top = Math.max(...data!.spend.byAgent.map((x) => x.cost), 0.0001);
                return (
                  <div key={a.name} className="flex items-center gap-3">
                    <div className="w-32 shrink-0 truncate text-sm">{a.name}</div>
                    <div className="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-panel-2">
                      <div
                        style={{ width: `${(a.cost / top) * 100}%` }}
                        className="h-full rounded-full bg-accent transition-[width] duration-500 ease-out"
                      />
                    </div>
                    <div className="w-16 shrink-0 text-right text-xs tabular-nums text-ink-dim">
                      ${a.cost.toFixed(2)}
                    </div>
                  </div>
                );
              })}
            </div>
          </div>
        )}
      </Section>

      <AnimatePresence>
        {orgRun && <OrgRunView runId={orgRun} onClose={() => setOrgRun(null)} />}
      </AnimatePresence>
    </div>
  );
}

/** Set or clear the daily cap. Lives inside the spend card because the number
 *  it governs is right there — a cap in a settings page elsewhere is a cap
 *  nobody sets. */
function BudgetControl({
  current,
  onSaved,
}: {
  current: number | null;
  onSaved: () => void;
}) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");

  async function save(cap: number | null) {
    await api.setBudget(cap);
    setEditing(false);
    onSaved();
  }

  if (!editing) {
    return (
      <button
        onClick={() => {
          setValue(current ? String(current) : "");
          setEditing(true);
        }}
        className="rounded-lg border border-line px-2.5 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
      >
        {current ? `Daily cap $${current.toFixed(2)} — change` : "Set a daily cap"}
      </button>
    );
  }

  return (
    <form
      onSubmit={(e) => {
        e.preventDefault();
        const n = parseFloat(value);
        save(Number.isFinite(n) && n > 0 ? n : null);
      }}
      className="flex items-center gap-1.5"
    >
      <span className="text-xs text-ink-dim">$</span>
      <input
        autoFocus
        value={value}
        onChange={(e) => setValue(e.target.value)}
        inputMode="decimal"
        placeholder="25"
        className="w-20 rounded-lg border border-line bg-panel px-2 py-1 text-xs outline-none focus:border-accent"
      />
      <button
        type="submit"
        className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white"
      >
        Save
      </button>
      {current != null && (
        <button
          type="button"
          onClick={() => save(null)}
          className="rounded-lg border border-line px-2.5 py-1 text-xs text-ink-dim hover:border-danger hover:text-danger"
        >
          Remove
        </button>
      )}
      <button
        type="button"
        onClick={() => setEditing(false)}
        className="px-1 text-xs text-ink-dim hover:text-ink"
      >
        ✕
      </button>
    </form>
  );
}

/** Opt-in to browser notifications. The prompt has to hang off a click, so
 *  this can't just be a setting read at startup. */
function NotifyToggle() {
  const [on, setOn] = useState(false);
  useEffect(() => setOn(notificationsOn()), []);

  const blocked = typeof Notification !== "undefined" && Notification.permission === "denied";

  return (
    <button
      onClick={async () => setOn(await toggleNotifications(!on))}
      disabled={blocked}
      title={
        blocked
          ? "Notifications are blocked for this site in your browser settings"
          : "Get a browser notification when a run needs you"
      }
      className={`rounded-lg border px-3 py-1.5 text-sm transition-colors disabled:opacity-50 ${
        on ? "border-accent bg-accent/5 text-accent" : "border-line bg-panel hover:bg-panel-2"
      }`}
    >
      {on ? "🔔 Notifications on" : "🔕 Notify me"}
    </button>
  );
}

function RunRow({ run, onOpenOrg }: { run: ActivityRun; onOpenOrg: () => void }) {
  const body = (
    <>
      <span
        className="h-2 w-2 shrink-0 rounded-full"
        style={{ background: statusColor(run.status) }}
      />
      {isWorking(run.status) && (
        <motion.span
          className="-ml-4 h-2 w-2 shrink-0 rounded-full"
          style={{ background: statusColor(run.status) }}
          animate={{ scale: [1, 2.4], opacity: [0.5, 0] }}
          transition={{ duration: 1.6, repeat: Infinity, ease: "easeOut" }}
        />
      )}
      <div className="min-w-0 flex-1">
        <div className="truncate text-sm font-medium">{run.label}</div>
        <div className="truncate text-xs text-ink-dim">
          {[run.projectName, run.teamName, statusLabel(run.status)]
            .filter(Boolean)
            .join(" · ")}
        </div>
      </div>
      <Elapsed since={run.startedAt ?? run.createdAt} />
      {run.costUsd != null && (
        <span className="w-14 shrink-0 text-right text-xs tabular-nums text-ink-dim">
          ${run.costUsd.toFixed(2)}
        </span>
      )}
    </>
  );

  const className =
    "card-shadow flex w-full items-center gap-3 rounded-xl border border-line bg-panel px-4 py-3 text-left hover:bg-panel-2";

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
    >
      {run.isOrg ? (
        <button onClick={onOpenOrg} className={className}>
          {body}
        </button>
      ) : run.projectId ? (
        <Link to={`/projects/${run.projectId}`} className={className}>
          {body}
        </Link>
      ) : (
        <div className={className}>{body}</div>
      )}
    </motion.div>
  );
}

/** Ticking elapsed time. A run that has been going 40 minutes should look
 *  different from one that started 10 seconds ago. */
function Elapsed({ since }: { since: string }) {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const timer = setInterval(() => setNow(Date.now()), 1000);
    return () => clearInterval(timer);
  }, []);

  const secs = Math.max(0, Math.floor((now - new Date(since).getTime()) / 1000));
  const text =
    secs < 60
      ? `${secs}s`
      : secs < 3600
        ? `${Math.floor(secs / 60)}m ${secs % 60}s`
        : `${Math.floor(secs / 3600)}h ${Math.floor((secs % 3600) / 60)}m`;
  return (
    <span className="w-16 shrink-0 text-right text-xs tabular-nums text-ink-dim">{text}</span>
  );
}

function Bars({ daily }: { daily: { day: string; cost: number; runs: number }[] }) {
  // The API only returns days that had runs. Plotting those alone turns two
  // busy days into two half-width bars that read as a single line and hides
  // the fact that nothing happened in between — so the axis is always the
  // full window, quiet days included.
  const byDay = new Map(daily.map((d) => [d.day.slice(0, 10), d]));
  const days = Array.from({ length: 14 }, (_, i) => {
    const date = new Date();
    date.setDate(date.getDate() - (13 - i));
    const key = date.toISOString().slice(0, 10);
    return byDay.get(key) ?? { day: key, cost: 0, runs: 0 };
  });
  const top = Math.max(...days.map((d) => d.cost), 0.0001);

  return (
    <div className="mt-4">
      <div className="flex h-24 items-end gap-1.5">
        {days.map((d) => (
          <div key={d.day} className="group relative flex-1">
            {/* A CSS transition rather than a motion value. A JS-driven
                animation writes the *current* frame to the element, and
                rAF is suspended in a background tab — so the bars freeze
                part-grown and the chart reads as wrong data rather than as
                an unfinished animation. With a transition the final height
                is in the style immediately and only the approach animates. */}
            <div
              style={{ height: d.cost > 0 ? Math.max(3, (d.cost / top) * 96) : 2 }}
              className={`w-full rounded-t transition-[height] duration-500 ease-out ${
                d.cost > 0 ? "bg-accent/70 group-hover:bg-accent" : "bg-line"
              }`}
            />
            <div className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 hidden -translate-x-1/2 whitespace-nowrap rounded bg-ink px-2 py-1 text-[11px] text-white group-hover:block">
              {formatDay(d.day)} · ${d.cost.toFixed(2)} · {d.runs} run
              {d.runs === 1 ? "" : "s"}
            </div>
          </div>
        ))}
      </div>
      <div className="mt-1.5 flex justify-between text-[11px] text-ink-dim">
        <span>{formatDay(days[0].day)}</span>
        <span>today</span>
      </div>
    </div>
  );
}

function formatDay(day: string): string {
  return new Date(day).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <>
      <h2 className="mt-10 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        {title}
      </h2>
      <div className="mt-3 max-w-3xl">{children}</div>
    </>
  );
}

function Stat({ label, value, accent }: { label: string; value: string; accent: string }) {
  return (
    <div className="card-shadow rounded-xl border border-line bg-panel p-4">
      <div className="text-2xl font-bold" style={{ color: accent }}>
        {value}
      </div>
      <div className="mt-1 text-xs text-ink-dim">{label}</div>
    </div>
  );
}
