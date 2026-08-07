import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { ActivityRun, api, Blocker } from "../lib/api";
import { useEngines } from "../lib/engines";
import { useActivity, notificationsOn, toggleNotifications } from "../lib/activity";
import { useWorkspace } from "../lib/workspace";
import { isWorking, statusColor, statusLabel } from "../lib/runStatus";
import { OrgRunView } from "../components/orgs/OrgRunView";
import { PermissionRow } from "../components/PermissionRow";
import { ActivityLine } from "../components/RunStream";
import { Stat } from "../components/Stat";
import { SpendBars } from "../components/spend/SpendBars";
import { SpendPanel } from "../components/spend/SpendPanel";
import { UsagePanel } from "../components/usage/UsagePanel";
import { useRunStream } from "../lib/ws";

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

  // Runs stopped from this page, masking what the 4s poll still reports.
  //
  // Without it, pressing stop leaves the row sitting there for up to four
  // seconds looking exactly as it did — which reads as "the button did
  // nothing", and invites a second press.
  const [stopped, setStopped] = useState<Set<string>>(new Set());
  const stop = (runId: string) => {
    setStopped((s) => new Set(s).add(runId));
    load();
  };

  const live = (data?.live ?? []).filter((r) => !stopped.has(r.id));
  const working = live.filter((r) => isWorking(r.status));
  const queued = live.filter((r) => r.status === "queued");
  const blocked = (data?.blocked ?? []).filter((b) => !stopped.has(b.runId));

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

      <div className="mt-6 grid max-w-3xl grid-cols-[repeat(2,minmax(0,1fr))] gap-4 sm:grid-cols-[repeat(4,minmax(0,1fr))]">
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
                <PlanBlocker
                  key={`${b.runId}-plan-${i}`}
                  blocker={b}
                  onOpenOrg={() => setOrgRun(b.runId)}
                  onStopped={() => stop(b.runId)}
                />
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
                <RunRow
                  key={run.id}
                  run={run}
                  onOpenOrg={() => setOrgRun(run.id)}
                  onStopped={() => stop(run.id)}
                />
              ))}
            </AnimatePresence>
          </div>
        )}
      </Section>

      <Section title="Claude plan usage">
        <UsagePanel />
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
          <div className="mt-4">
            <SpendBars daily={data?.spend.daily ?? []} />
          </div>
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

        <SpendPanel />
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

/**
 * Stop a run, from the page that shows you it is running.
 *
 * Two presses rather than one. Everything on this page is long-lived and
 * expensive — the whole reason to look at it is that something has been going
 * for hours — and a single misplaced click throwing away that work, next to a
 * row you clicked to *open*, is not a trade worth making.
 */
function StopRun({
  runId,
  what,
  onStopped,
}: {
  runId: string;
  /** Named in the confirmation, so it is obvious which row is about to stop. */
  what: string;
  onStopped: () => void;
}) {
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (error) {
    return (
      <span className="shrink-0 text-[11px] text-danger" title={error}>
        couldn’t stop it
      </span>
    );
  }

  if (!confirming) {
    return (
      <button
        onClick={() => setConfirming(true)}
        title={`Stop ${what}`}
        aria-label={`Stop ${what}`}
        className="shrink-0 rounded-lg px-2 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-danger"
      >
        ✕
      </button>
    );
  }

  return (
    <span className="flex shrink-0 items-center gap-1">
      <button
        disabled={busy}
        onClick={async () => {
          setBusy(true);
          try {
            await api.cancelRun(runId);
            // Only after it worked: the row disappears on success, so an error
            // shown in its place needs the row to still be there.
            onStopped();
          } catch (e) {
            setError(String(e).replace(/^Error:\s*/, ""));
          }
        }}
        className="rounded-lg bg-danger px-2.5 py-1 text-xs font-medium text-white disabled:opacity-60"
      >
        {busy ? "Stopping…" : "Stop it"}
      </button>
      <button
        onClick={() => setConfirming(false)}
        className="rounded-lg px-2 py-1 text-xs text-ink-dim hover:text-ink"
      >
        Keep going
      </button>
    </span>
  );
}

function RunRow({
  run,
  onOpenOrg,
  onStopped,
}: {
  run: ActivityRun;
  onOpenOrg: () => void;
  onStopped: () => void;
}) {
  const engines = useEngines();
  // Naming the engine only earns its space once there's more than one; the
  // model always does.
  const engineLabel =
    (engines?.length ?? 0) > 1
      ? engines?.find((e) => e.id === run.engine)?.label
      : undefined;
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
          {[
            run.projectName,
            run.teamName,
            statusLabel(run.status),
            // Which CLI is spending your subscription, and on what. Only
            // worth the space once more than one engine exists.
            [engineLabel, run.model].filter(Boolean).join(" · ") || null,
          ]
            .filter(Boolean)
            .join(" · ")}
        </div>
        {/* The operations view is exactly where "running" is too vague. */}
        {isWorking(run.status) && <LiveAction runId={run.id} />}
      </div>
      <Elapsed since={run.startedAt ?? run.createdAt} />
      {run.costUsd != null && (
        <span className="w-14 shrink-0 text-right text-xs tabular-nums text-ink-dim">
          ${run.costUsd.toFixed(2)}
        </span>
      )}
    </>
  );

  // The card is the container and the clickable region sits inside it, rather
  // than the card *being* a link. Stop has to live on the row, and a button
  // nested inside a link is neither valid nor operable by keyboard.
  const open = "flex min-w-0 flex-1 items-center gap-3 text-left";

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      className="card-shadow flex items-center gap-2 rounded-xl border border-line bg-panel py-3 pl-4 pr-2"
    >
      {run.isOrg ? (
        <button onClick={onOpenOrg} className={open}>
          {body}
        </button>
      ) : run.projectId ? (
        <Link to={`/projects/${run.projectId}`} className={open}>
          {body}
        </Link>
      ) : (
        <div className={open}>{body}</div>
      )}
      <StopRun runId={run.id} what={run.label} onStopped={onStopped} />
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

/** Current action for one live run. Its own component so the socket exists
 *  only while the run does. */
function LiveAction({ runId }: { runId: string }) {
  const events = useRunStream(runId);
  return <ActivityLine events={events} live />;
}

/**
 * A plan waiting on you. A team's opens the room it was written in; a card's
 * opens the board it belongs to, where the drawer shows it inline.
 */
function PlanBlocker({
  blocker,
  onOpenOrg,
  onStopped,
}: {
  blocker: Blocker;
  onOpenOrg: () => void;
  onStopped: () => void;
}) {
  const open = "flex min-w-0 flex-1 items-center gap-3 text-left";
  const body = (
    <>
      <span className="text-lg">◧</span>
      <div className="min-w-0">
        <div className="truncate text-sm font-semibold">{blocker.label}</div>
        <div className="truncate text-xs text-ink-dim">
          A plan is ready for your review
        </div>
      </div>
    </>
  );
  return (
    <motion.div
      initial={{ opacity: 0, x: -6 }}
      animate={{ opacity: 1, x: 0 }}
      className="card-shadow flex items-center gap-2 rounded-xl border border-amber-300 bg-amber-50/60 py-3 pl-4 pr-2"
    >
      {blocker.isOrg ? (
        <button onClick={onOpenOrg} className={open}>
          {body}
        </button>
      ) : blocker.projectId ? (
        <Link to={`/projects/${blocker.projectId}`} className={open}>
          {body}
        </Link>
      ) : (
        <div className={open}>{body}</div>
      )}
      {/* Turning a plan down is a real answer to "waiting on you", and the one
          this row could not give. Reviewing it elsewhere and never coming back
          is how a run sits here for seven hours. */}
      <StopRun runId={blocker.runId} what={blocker.label} onStopped={onStopped} />
    </motion.div>
  );
}
