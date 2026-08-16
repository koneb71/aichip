import { useCallback, useEffect, useMemo, useState } from "react";
import { Link } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, Manager, ManagerPass, api } from "../lib/api";
import { Preset, WEEKDAYS, compile, describeCron, recognize, relative } from "../lib/cron";
import { Icon } from "./ui/Icon";

/**
 * Assign someone to run this board while you are not looking.
 *
 * The panel is built around the one question a person actually has about an
 * unattended agent — *what did it do* — so the pass history is not a footnote
 * here the way a firing log is on the Routines page. It is the reason to open
 * the tab.
 *
 * Everything above it exists to answer "and what may it do next": who is
 * managing, how often, and the cap on cards per pass. The cap is the setting
 * that decides whether this feature is comfortable to leave on, so it is a
 * control and a sentence, not a number in a corner.
 */
export function ManagerPanel({
  projectId,
  workspaceId,
}: {
  projectId: string;
  workspaceId?: string;
}) {
  const [agents, setAgents] = useState<Agent[]>([]);
  const [manager, setManager] = useState<Manager | null>(null);
  const [passes, setPasses] = useState<ManagerPass[]>([]);
  const [loaded, setLoaded] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Draft state. Seeded from the saved manager once it arrives.
  const [agentId, setAgentId] = useState<string>("");
  const [brief, setBrief] = useState("");
  const [preset, setPreset] = useState<Preset>("daily");
  const [time, setTime] = useState("09:00");
  const [weekday, setWeekday] = useState(1);
  const [monthday, setMonthday] = useState(1);
  const [custom, setCustom] = useState("0 9 * * *");
  const [maxStarts, setMaxStarts] = useState(2);
  const [nextThree, setNextThree] = useState<string[]>([]);

  const cronExpr = compile(preset, time, weekday, monthday, custom);

  const load = useCallback(async () => {
    const [m, p] = await Promise.all([
      api.manager(projectId).catch(() => ({ manager: null })),
      api.managerPasses(projectId).catch(() => ({ passes: [] })),
    ]);
    setManager(m.manager);
    setPasses(p.passes);
    setLoaded(true);
    return m.manager;
  }, [projectId]);

  useEffect(() => {
    if (!workspaceId) return;
    api
      .agents(workspaceId)
      .then((r) => setAgents(r.agents))
      .catch(() => {});
  }, [workspaceId]);

  useEffect(() => {
    load().then((m) => {
      if (!m) return;
      const r = recognize(m.cronExpr);
      setAgentId(m.agentId ?? "");
      setBrief(m.brief);
      setPreset(r.preset);
      setTime(r.time);
      setWeekday(r.weekday);
      setMonthday(r.monthday);
      setCustom(m.cronExpr);
      setMaxStarts(m.maxStarts);
    });
  }, [load]);

  // While a pass is in flight the history is stale the moment it renders, so
  // poll — but only then. An idle manager is the common case and does not
  // deserve a request every two seconds forever.
  const live = passes.some((p) => ["queued", "starting", "running"].includes(p.runStatus ?? ""));
  useEffect(() => {
    if (!live) return;
    const t = setInterval(() => {
      api.managerPasses(projectId).then((r) => setPasses(r.passes)).catch(() => {});
    }, 2500);
    return () => clearInterval(t);
  }, [live, projectId]);

  // The next firings come from the server's own croner — the one that will
  // actually fire it — rather than a lookalike in JS.
  useEffect(() => {
    let stale = false;
    api
      .routinePreview(cronExpr)
      .then((r) => !stale && setNextThree(r.next))
      .catch(() => !stale && setNextThree([]));
    return () => {
      stale = true;
    };
  }, [cronExpr]);

  const dirty = useMemo(() => {
    if (!manager) return true;
    return (
      (manager.agentId ?? "") !== agentId ||
      manager.brief !== brief.trim() ||
      manager.cronExpr !== cronExpr ||
      manager.maxStarts !== maxStarts
    );
  }, [manager, agentId, brief, cronExpr, maxStarts]);

  const run = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      await load();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const save = () =>
    run(() =>
      api.managerSave(projectId, {
        agentId: agentId || null,
        brief: brief.trim(),
        cronExpr,
        maxStarts,
        enabled: manager?.enabled ?? true,
      }),
    );

  if (!loaded) {
    return <div className="p-6 text-sm text-ink-dim">Loading…</div>;
  }

  return (
    <div className="mx-auto max-w-3xl space-y-5 p-4">
      {/* Who, and whether they are on duty. */}
      <section className="rounded-2xl border border-line bg-panel p-4">
        <div className="flex items-start justify-between gap-3">
          <div>
            <h2 className="text-sm font-semibold">Project manager</h2>
            <p className="mt-0.5 max-w-lg text-xs text-ink-dim">
              An agent that reviews this board on a schedule and acts on it while nobody is
              watching — reading what finished, filing what is done, and starting what it
              judges should happen next, within a cap you set.
            </p>
          </div>
          {manager && (
            <button
              onClick={() =>
                run(() =>
                  api.managerSave(projectId, {
                    agentId: manager.agentId,
                    brief: manager.brief,
                    cronExpr: manager.cronExpr,
                    maxStarts: manager.maxStarts,
                    enabled: !manager.enabled,
                  }),
                )
              }
              disabled={busy}
              className={`ring-focus shrink-0 rounded-lg border px-2.5 py-1 text-xs transition-colors disabled:opacity-50 ${
                manager.enabled
                  ? "border-accent bg-accent/10 font-medium text-accent"
                  : "border-line text-ink-dim hover:border-accent/50"
              }`}
            >
              {manager.enabled ? "On duty" : "Off duty"}
            </button>
          )}
        </div>

        <div className="mt-4 space-y-3">
          <label className="block">
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              Who manages this project
            </span>
            <select
              value={agentId}
              onChange={(e) => setAgentId(e.target.value)}
              className="w-full rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
            >
              <option value="">The assistant, with no particular persona</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <span className="mt-1 block text-[11px] text-ink-dim">
              The agent's own instructions shape how it manages. It does not become the
              agent that writes the code — the manager picks that per card.
            </span>
          </label>

          <label className="block">
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              What it should care about
            </span>
            <textarea
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              rows={3}
              placeholder="Bugs before features. Keep the test suite green. Don't touch the payments code without asking."
              className="w-full resize-y rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
            />
          </label>

          {/* The schedule builder, same shapes the Routines editor writes. */}
          <div>
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              How often
            </span>
            <div className="flex flex-wrap items-center gap-2">
              <select
                value={preset}
                onChange={(e) => setPreset(e.target.value as Preset)}
                className="rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
              >
                <option value="hourly">Every hour</option>
                <option value="daily">Every day</option>
                <option value="weekdays">Weekdays</option>
                <option value="weekly">Weekly</option>
                <option value="monthly">Monthly</option>
                <option value="custom">Custom…</option>
              </select>
              {preset !== "hourly" && preset !== "custom" && (
                <input
                  type="time"
                  value={time}
                  onChange={(e) => setTime(e.target.value)}
                  className="rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                />
              )}
              {preset === "weekly" && (
                <select
                  value={weekday}
                  onChange={(e) => setWeekday(parseInt(e.target.value, 10))}
                  className="rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                >
                  {WEEKDAYS.map((d, i) => (
                    <option key={d} value={i}>
                      {d}
                    </option>
                  ))}
                </select>
              )}
              {preset === "monthly" && (
                <input
                  type="number"
                  min={1}
                  max={28}
                  value={monthday}
                  onChange={(e) => setMonthday(parseInt(e.target.value, 10) || 1)}
                  className="w-20 rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                />
              )}
              {preset === "custom" && (
                <input
                  value={custom}
                  onChange={(e) => setCustom(e.target.value)}
                  placeholder="0 9 * * *"
                  className="w-40 rounded-lg border border-line bg-panel px-2.5 py-1.5 font-mono text-sm outline-none focus:border-accent"
                />
              )}
            </div>
            <p className="mt-1 text-[11px] text-ink-dim">
              {nextThree.length > 0
                ? `Next: ${nextThree.slice(0, 2).map((t) => new Date(t).toLocaleString()).join(", ")}`
                : "That isn't a schedule this can read."}
            </p>
          </div>

          {/* The setting that decides whether this is comfortable to leave on. */}
          <div>
            <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              Cards it may start per pass
            </span>
            <div className="flex items-center gap-3">
              <input
                type="range"
                min={0}
                max={10}
                value={maxStarts}
                onChange={(e) => setMaxStarts(parseInt(e.target.value, 10))}
                className="w-48"
              />
              <span className="w-6 text-sm font-medium tabular-nums">{maxStarts}</span>
            </div>
            <p className="mt-1 max-w-lg text-[11px] text-ink-dim">
              {maxStarts === 0
                ? "It will review, file and report, but never start anything. Cards it thinks should run wait in the backlog for you."
                : `A hard limit, counted server-side — not a request. Past ${maxStarts}, the ${
                    maxStarts === 1 ? "next card" : "rest"
                  } stays in the backlog and it says so in its summary. Cards imported from GitHub are never started by the manager, whatever this says.`}
            </p>
          </div>

          {error && (
            <div className="rounded-lg border border-danger/40 bg-danger/5 px-2.5 py-1.5 text-xs text-danger">
              {error}
            </div>
          )}

          <div className="flex flex-wrap items-center gap-2 border-t border-line pt-3">
            <button
              onClick={save}
              disabled={busy || (!dirty && !!manager)}
              className="ring-focus rounded-lg bg-accent px-3 py-1.5 text-xs text-white disabled:opacity-40"
            >
              {manager ? "Save" : "Assign a manager"}
            </button>
            {manager && (
              <>
                <button
                  onClick={() => run(() => api.managerRunNow(projectId))}
                  disabled={busy}
                  className="ring-focus rounded-lg border border-line px-3 py-1.5 text-xs hover:border-accent hover:text-accent disabled:opacity-40"
                >
                  Run a pass now
                </button>
                {manager.chatId && (
                  <Link
                    to={`/chat?project=${projectId}&chat=${manager.chatId}`}
                    className="ring-focus rounded-lg border border-line px-3 py-1.5 text-xs hover:border-accent hover:text-accent"
                  >
                    Open its thread
                  </Link>
                )}
                <button
                  onClick={() => run(() => api.managerRemove(projectId))}
                  disabled={busy}
                  className="ring-focus ml-auto rounded-lg px-3 py-1.5 text-xs text-ink-dim hover:text-danger disabled:opacity-40"
                >
                  Unassign
                </button>
              </>
            )}
          </div>
          {manager?.enabled && manager.nextAt && (
            <p className="text-[11px] text-ink-dim">
              <Icon name="clock" className="mr-1 inline size-3" />
              {describeCron(manager.cronExpr)} — next {relative(manager.nextAt)}
            </p>
          )}
        </div>
      </section>

      {/* The reason to open this tab. */}
      {manager && (
        <section>
          <h3 className="mb-2 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            What it has done
          </h3>
          {passes.length === 0 ? (
            <p className="rounded-2xl border border-dashed border-line px-4 py-6 text-center text-xs text-ink-dim">
              No passes yet. It will run {describeCron(manager.cronExpr)} — or press “Run a
              pass now” to watch one.
            </p>
          ) : (
            <div className="space-y-2">
              <AnimatePresence initial={false}>
                {passes.map((p) => (
                  <motion.div
                    key={p.id}
                    layout
                    initial={{ opacity: 0, y: -4 }}
                    animate={{ opacity: 1, y: 0 }}
                    className="rounded-xl border border-line bg-panel p-3"
                  >
                    <div className="flex flex-wrap items-baseline gap-2">
                      <span className="text-xs font-medium">
                        {new Date(p.firedAt).toLocaleString()}
                      </span>
                      {p.trigger === "manual" && (
                        <span className="rounded bg-panel-2 px-1.5 text-[10px] text-ink-dim">
                          by hand
                        </span>
                      )}
                      {p.runStatus && (
                        <span className="text-[10px] text-ink-dim">{p.runStatus}</span>
                      )}
                      {typeof p.costUsd === "number" && (
                        <span className="text-[10px] text-ink-dim tabular-nums">
                          ${p.costUsd.toFixed(2)}
                        </span>
                      )}
                    </div>
                    {p.error ? (
                      <p className="mt-1 text-xs text-danger">{p.error}</p>
                    ) : p.actions.length === 0 ? (
                      // Said out loud rather than left blank: "it looked and
                      // decided nothing needed doing" is a real outcome, and an
                      // empty row reads as a failure.
                      <p className="mt-1 text-xs text-ink-dim">
                        Nothing to change — read its thread for what it found.
                      </p>
                    ) : (
                      <ul className="mt-1.5 space-y-1">
                        {p.actions.map((a, i) => (
                          <li key={i} className="flex items-baseline gap-1.5 text-xs">
                            <span
                              className={`shrink-0 rounded px-1.5 text-[10px] ${
                                a.kind === "start"
                                  ? "bg-accent/10 text-accent"
                                  : a.kind === "cancel"
                                    ? "bg-danger/10 text-danger"
                                    : "bg-panel-2 text-ink-dim"
                              }`}
                            >
                              {a.kind === "create"
                                ? "filed"
                                : a.kind === "move"
                                  ? `→ ${a.detail}`
                                  : a.kind}
                            </span>
                            {a.taskId ? (
                              <Link
                                to={`/projects/${projectId}?task=${a.taskId}`}
                                className="truncate hover:text-accent hover:underline"
                              >
                                {a.title}
                              </Link>
                            ) : (
                              <span className="truncate text-ink-dim">{a.title}</span>
                            )}
                          </li>
                        ))}
                      </ul>
                    )}
                  </motion.div>
                ))}
              </AnimatePresence>
            </div>
          )}
        </section>
      )}
    </div>
  );
}
