import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Link } from "react-router-dom";
import { api, Project, Routine, RoutineDraft, RoutineRun } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { EnginePicker } from "../lib/engines";
import { Icon } from "../components/ui/Icon";

/**
 * Routines: a prompt that runs on a schedule.
 *
 * Three kinds, each landing where that work naturally lives — a chat turn in
 * the routine's standing thread, a fresh research report, or a board card
 * started on its project. The page is honest in both directions: the "next"
 * time comes from the same cron parser that will fire it, and the history
 * shows every firing including the ones that produced nothing and why.
 */

const KIND_LABEL: Record<Routine["kind"], string> = {
  chat: "Chat",
  research: "Research",
  task: "Task",
  watch: "Watch",
};
const KIND_BLURB: Record<Routine["kind"], string> = {
  chat: "replies collect in one thread",
  research: "a fresh cited report each time",
  task: "a card started on the board",
  watch: "check a page for changes",
};

/** The value the scope picker uses for "no project". */
const GENERAL = "general";

export default function RoutinesPage() {
  const { active } = useWorkspace();
  const workspaceId = active?.id ?? null;

  const [routines, setRoutines] = useState<Routine[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [editing, setEditing] = useState<RoutineDraft | null>(null);
  const [editId, setEditId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!workspaceId) return;
    api
      .routines(workspaceId)
      .then((r) => setRoutines(r.routines))
      .catch(() => {});
  }, [workspaceId]);

  useEffect(() => {
    refresh();
    if (!workspaceId) return;
    // "chat" scope = repositories + spaces: the widest set a routine can
    // attach to. The editor narrows per kind.
    api
      .projects(workspaceId, "chat")
      .then((r) => setProjects(r.projects))
      .catch(() => {});
  }, [workspaceId, refresh]);

  // A routine that just fired shows "running" — keep the list fresh while
  // anything is live, without a socket subscription per row.
  useEffect(() => {
    const anyLive = routines.some(
      (r) => r.lastRunStatus && !["completed", "failed", "canceled"].includes(r.lastRunStatus),
    );
    if (!anyLive) return;
    const t = setInterval(refresh, 3000);
    return () => clearInterval(t);
  }, [routines, refresh]);

  const startNew = () =>
    setEditing({
      name: "",
      kind: "chat",
      projectId: null,
      prompt: "",
      cronExpr: "0 9 * * 1-5",
      catchUp: "run_once",
    });

  const save = async (draft: RoutineDraft) => {
    if (!workspaceId) return;
    setError(null);
    try {
      if (editId) await api.routineUpdate(editId, draft);
      else await api.routineCreate(workspaceId, draft);
      setEditing(null);
      setEditId(null);
      refresh();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  return (
    <div className="mx-auto w-full max-w-3xl px-4 py-8 lg:px-8">
      <div className="flex items-center justify-between gap-3">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Routines</h1>
          <p className="mt-1 text-xs text-ink-dim">
            A prompt that runs on a schedule — a morning brief in chat, a weekly research
            report, a recurring card on a board. Times are your local time; if your machine
            was asleep, a missed routine runs once on wake.
          </p>
        </div>
        <button
          onClick={startNew}
          className="ring-focus flex shrink-0 items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] hover:brightness-110"
        >
          <Icon name="plus" size={14} strokeWidth={2.5} />
          New routine
        </button>
      </div>

      {error && (
        <div className="mt-4 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      {editing && (
        <Editor
          draft={editing}
          isNew={!editId}
          projects={projects}
          onCancel={() => {
            setEditing(null);
            setEditId(null);
          }}
          onSave={save}
        />
      )}

      <div className="mt-6 space-y-3">
        {routines.length === 0 && !editing && (
          <div className="rounded-2xl border border-dashed border-line px-6 py-10 text-center text-sm text-ink-dim">
            Nothing scheduled yet. A routine can post into a chat thread every morning,
            file a research report every Friday, or start a board card every Monday.
          </div>
        )}
        {routines.map((r) => (
          <RoutineCard
            key={r.id}
            routine={r}
            onChanged={refresh}
            onEdit={() => {
              setEditId(r.id);
              setEditing({
                name: r.name,
                kind: r.kind,
                projectId: r.projectId,
                prompt: r.prompt,
                url: r.url,
                cronExpr: r.cronExpr,
                catchUp: r.catchUp,
                engine: r.engine,
                modelTier: r.modelTier,
                effort: r.effort,
              });
              window.scrollTo({ top: 0, behavior: "smooth" });
            }}
            onError={(e) => setError(e)}
          />
        ))}
      </div>
    </div>
  );
}

function RoutineCard({
  routine: r,
  onChanged,
  onEdit,
  onError,
}: {
  routine: Routine;
  onChanged: () => void;
  onEdit: () => void;
  onError: (e: string) => void;
}) {
  const [open, setOpen] = useState(false);
  const [history, setHistory] = useState<RoutineRun[] | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!open) return;
    api
      .routineHistory(r.id)
      .then((h) => setHistory(h.runs))
      .catch(() => {});
  }, [open, r.id, r.lastFiredAt, r.lastRunStatus]);

  const runNow = async () => {
    setBusy(true);
    try {
      await api.routineRunNow(r.id);
      setOpen(true);
    } catch (e) {
      onError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
      onChanged();
    }
  };

  const toggle = async () => {
    try {
      await api.routineUpdate(r.id, { enabled: !r.enabled });
      onChanged();
    } catch (e) {
      onError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const remove = async () => {
    if (!window.confirm(`Delete "${r.name}"? What it already produced stays.`)) return;
    try {
      await api.routineDelete(r.id);
      onChanged();
    } catch (e) {
      onError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const live =
    r.lastRunStatus && !["completed", "failed", "canceled"].includes(r.lastRunStatus);

  return (
    <div className={`card-shadow rounded-2xl border border-line bg-panel p-4 ${r.enabled ? "" : "opacity-60"}`}>
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[10px] font-semibold uppercase tracking-wide text-ink-dim">
              {KIND_LABEL[r.kind]}
            </span>
            <span className="truncate text-sm font-semibold">{r.name}</span>
            {r.projectName && (
              <span className="truncate text-[11px] text-ink-dim">· {r.projectName}</span>
            )}
            {r.kind === "watch" && r.url && (
              <span className="truncate text-[11px] text-ink-dim" title={r.url}>
                · {hostOf(r.url)}
              </span>
            )}
            {!r.projectName && r.kind !== "task" && r.kind !== "watch" && (
              <span className="text-[11px] text-ink-dim">· General</span>
            )}
          </div>
          <p className="mt-1 line-clamp-2 text-xs text-ink-dim">{r.prompt}</p>
          <div className="mt-2 flex flex-wrap items-center gap-x-3 gap-y-1 text-[11px] text-ink-dim">
            <span className="flex items-center gap-1">
              <Icon name="clock" size={12} />
              {describeCron(r.cronExpr)}
            </span>
            {r.enabled && r.nextAt && <span>next {relative(r.nextAt)}</span>}
            {!r.enabled && <span>paused</span>}
            <LastOutcome routine={r} />
          </div>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <button
            onClick={runNow}
            disabled={busy || !!live}
            title={live ? "Already running" : "Fire once, without touching the schedule"}
            className="rounded-lg border border-line px-2.5 py-1 text-xs hover:border-ink-dim disabled:opacity-50"
          >
            {busy ? "Firing…" : "Run now"}
          </button>
          <button
            onClick={onEdit}
            className="rounded-lg border border-line px-2.5 py-1 text-xs hover:border-ink-dim"
          >
            Edit
          </button>
          {/* The switch: on = scheduled, off = paused, bookmark reset on re-enable. */}
          <button
            onClick={toggle}
            role="switch"
            aria-checked={r.enabled}
            title={r.enabled ? "Pause the schedule" : "Resume the schedule"}
            className={`relative h-5 w-9 rounded-full transition-colors ${r.enabled ? "bg-accent" : "bg-line"}`}
          >
            <span
              className={`absolute top-0.5 size-4 rounded-full bg-white shadow transition-[left] ${r.enabled ? "left-[18px]" : "left-0.5"}`}
            />
          </button>
        </div>
      </div>

      <div className="mt-2 flex items-center gap-3 text-[11px]">
        <button onClick={() => setOpen(!open)} className="text-ink-dim hover:text-ink">
          {open ? "Hide history" : "History"}
        </button>
        <ResultLink routine={r} />
        <button onClick={remove} className="ml-auto text-ink-dim hover:text-danger">
          Delete
        </button>
      </div>

      {open && (
        <div className="mt-2 space-y-1 border-t border-line pt-2">
          {history === null && <div className="text-[11px] text-ink-dim">Loading…</div>}
          {history?.length === 0 && (
            <div className="text-[11px] text-ink-dim">Hasn't fired yet.</div>
          )}
          {history?.map((h) => (
            <div key={h.id} className="flex flex-wrap items-center gap-2 text-[11px]">
              <span className="text-ink-dim">{new Date(h.firedAt).toLocaleString()}</span>
              {h.trigger === "manual" && <span className="text-ink-dim">(manual)</span>}
              {h.error ? (
                <span className="text-danger">didn't run: {h.error}</span>
              ) : (
                <>
                  <StatusDot status={h.runStatus} />
                  <FiringLink run={h} routine={r} />
                  {h.costUsd != null && (
                    <span className="text-ink-dim">${h.costUsd.toFixed(2)}</span>
                  )}
                </>
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function LastOutcome({ routine: r }: { routine: Routine }) {
  if (!r.lastFiredAt) return null;
  if (r.lastError) {
    return (
      <span className="text-danger" title={r.lastError}>
        last firing failed
      </span>
    );
  }
  if (r.lastRunStatus && !["completed", "failed", "canceled"].includes(r.lastRunStatus)) {
    return <span className="text-accent">running now</span>;
  }
  if (r.lastRunStatus === "failed") {
    return <span className="text-danger">last run failed</span>;
  }
  return <span>last ran {relative(r.lastFiredAt)}</span>;
}

/** Where this routine's output lives, one click away. */
function ResultLink({ routine: r }: { routine: Routine }) {
  if ((r.kind === "chat" || r.kind === "watch") && r.chatId) {
    return (
      <Link
        to={`/chat?project=${r.projectId ?? GENERAL}&chat=${r.chatId}`}
        className="text-accent hover:underline"
      >
        Open thread
      </Link>
    );
  }
  if (r.kind === "task" && r.projectId) {
    return (
      <Link to={`/projects/${r.projectId}`} className="text-accent hover:underline">
        Open board
      </Link>
    );
  }
  return null;
}

function FiringLink({ run: h, routine: r }: { run: RoutineRun; routine: Routine }) {
  if (h.researchId) {
    return (
      <Link to={`/research/${h.researchId}`} className="text-accent hover:underline">
        {h.researchTitle || "report"}
      </Link>
    );
  }
  if (h.taskId) {
    return (
      <Link to={`/projects/${h.taskProjectId ?? r.projectId}`} className="text-accent hover:underline">
        {h.taskTitle || "card"}
      </Link>
    );
  }
  if (h.chatId) {
    return (
      <Link
        to={`/chat?project=${r.projectId ?? GENERAL}&chat=${h.chatId}`}
        className="text-accent hover:underline"
      >
        thread
      </Link>
    );
  }
  return null;
}

function StatusDot({ status }: { status: string | null }) {
  const color =
    status === "completed"
      ? "bg-tier-easy"
      : status === "failed" || status === "canceled"
        ? "bg-danger"
        : "bg-amber-400";
  return <span className={`size-1.5 rounded-full ${color}`} title={status ?? "queued"} />;
}

/* ── The editor ─────────────────────────────────────────────────────────── */

type Preset = "hourly" | "daily" | "weekdays" | "weekly" | "monthly" | "custom";
const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

/** Compile the builder's fields into five-field cron. */
function compile(preset: Preset, time: string, weekday: number, monthday: number, custom: string): string {
  const [h, m] = time.split(":").map((n) => parseInt(n, 10));
  switch (preset) {
    case "hourly":
      return `${isNaN(m) ? 0 : m} * * * *`;
    case "daily":
      return `${m} ${h} * * *`;
    case "weekdays":
      return `${m} ${h} * * 1-5`;
    case "weekly":
      return `${m} ${h} * * ${weekday}`;
    case "monthly":
      return `${m} ${h} ${monthday} * *`;
    case "custom":
      return custom;
  }
}

/** Recognize the shapes the builder writes, so editing round-trips. */
function recognize(expr: string): { preset: Preset; time: string; weekday: number; monthday: number } {
  const m = expr.trim().match(/^(\d{1,2}) (\d{1,2}|\*) (\d{1,2}|\*) \* (\*|1-5|\d)$/);
  const fallback = { preset: "custom" as Preset, time: "09:00", weekday: 1, monthday: 1 };
  if (!m) return fallback;
  const [, min, hour, dom, dow] = m;
  const time =
    hour === "*" ? "09:00" : `${hour.padStart(2, "0")}:${min.padStart(2, "0")}`;
  if (hour === "*" && dom === "*" && dow === "*") return { ...fallback, preset: "hourly" };
  if (dom === "*" && dow === "*") return { ...fallback, preset: "daily", time };
  if (dom === "*" && dow === "1-5") return { ...fallback, preset: "weekdays", time };
  if (dom === "*" && /^\d$/.test(dow))
    return { ...fallback, preset: "weekly", time, weekday: parseInt(dow, 10) };
  if (dom !== "*" && dow === "*")
    return { ...fallback, preset: "monthly", time, monthday: parseInt(dom, 10) };
  return fallback;
}

/** The schedule in words, for the card. */
function describeCron(expr: string): string {
  const r = recognize(expr);
  switch (r.preset) {
    case "hourly":
      return "every hour";
    case "daily":
      return `every day at ${r.time}`;
    case "weekdays":
      return `weekdays at ${r.time}`;
    case "weekly":
      return `${WEEKDAYS[r.weekday]}s at ${r.time}`;
    case "monthly":
      return `monthly on day ${r.monthday} at ${r.time}`;
    case "custom":
      return expr;
  }
}

function relative(iso: string): string {
  const ms = new Date(iso).getTime() - Date.now();
  const abs = Math.abs(ms);
  const mins = Math.round(abs / 60000);
  const text =
    mins < 1
      ? "under a minute"
      : mins < 60
        ? `${mins} min`
        : mins < 60 * 48
          ? `${Math.round(mins / 60)} h`
          : `${Math.round(mins / 1440)} days`;
  return ms >= 0 ? `in ${text}` : `${text} ago`;
}

function Editor({
  draft,
  isNew,
  projects,
  onCancel,
  onSave,
}: {
  draft: RoutineDraft;
  isNew: boolean;
  projects: Project[];
  onCancel: () => void;
  onSave: (d: RoutineDraft) => void;
}) {
  const [d, setD] = useState(draft);
  const seed = useMemo(() => recognize(draft.cronExpr), [draft.cronExpr]);
  const [preset, setPreset] = useState<Preset>(seed.preset);
  const [time, setTime] = useState(seed.time);
  const [weekday, setWeekday] = useState(seed.weekday);
  const [monthday, setMonthday] = useState(seed.monthday);
  const [custom, setCustom] = useState(seed.preset === "custom" ? draft.cronExpr : "0 9 * * *");
  const [preview, setPreview] = useState<{ valid: boolean; next: string[] } | null>(null);

  const cronExpr = compile(preset, time, weekday, monthday, custom);

  // The preview asks the server — the same croner that will fire it — and is
  // debounced so typing a custom expression doesn't spam.
  const previewTimer = useRef<ReturnType<typeof setTimeout>>(undefined);
  useEffect(() => {
    clearTimeout(previewTimer.current);
    previewTimer.current = setTimeout(() => {
      api
        .routinePreview(cronExpr)
        .then(setPreview)
        .catch(() => setPreview(null));
    }, 250);
    return () => clearTimeout(previewTimer.current);
  }, [cronExpr]);

  // Which projects this kind can attach to. A task needs a board (repos); a
  // chat can also stand in a document space; research is repos or general.
  const eligible = projects.filter((p) =>
    d.kind === "chat" ? true : (p as { kind?: string }).kind !== "space",
  );
  const allowGeneral = d.kind !== "task";

  const set = (patch: Partial<RoutineDraft>) => setD((prev) => ({ ...prev, ...patch }));

  const field = "rounded-lg border border-line bg-panel px-2.5 py-1.5 text-xs";

  return (
    <div className="card-shadow mt-5 rounded-2xl border border-line bg-panel p-4">
      <div className="text-sm font-semibold">{isNew ? "New routine" : `Edit ${draft.name}`}</div>

      <div className="mt-3 grid gap-3">
        <input
          value={d.name}
          onChange={(e) => set({ name: e.target.value })}
          placeholder="Name — e.g. Morning brief"
          className={`${field} w-full`}
        />

        {/* What a firing produces. Explained in place, because the choice is
            the whole feature. */}
        <div className="flex flex-wrap gap-2">
          {(Object.keys(KIND_LABEL) as Routine["kind"][]).map((k) => (
            <button
              key={k}
              onClick={() => set({ kind: k })}
              className={`rounded-xl border px-3 py-1.5 text-left text-xs ${
                d.kind === k ? "border-accent bg-accent/5" : "border-line hover:border-ink-dim/40"
              }`}
            >
              <div className="font-semibold">{KIND_LABEL[k]}</div>
              <div className="text-[10px] text-ink-dim">{KIND_BLURB[k]}</div>
            </button>
          ))}
        </div>

        {d.kind === "watch" ? (
          <input
            value={d.url ?? ""}
            onChange={(e) => set({ url: e.target.value })}
            placeholder="https://the-page-to-watch.example/jobs"
            className={`${field} w-full font-mono`}
          />
        ) : (
          <div className="flex flex-wrap items-center gap-2">
            <select
              value={d.projectId ?? GENERAL}
              onChange={(e) => set({ projectId: e.target.value === GENERAL ? null : e.target.value })}
              className={field}
            >
              {/* Without a "general" option (the task kind) the browser would
                  display the first project while the state still says null —
                  a select must never show a choice nobody made. */}
              {allowGeneral ? (
                <option value={GENERAL}>General — no project</option>
              ) : (
                <option value={GENERAL} disabled>
                  Pick a project…
                </option>
              )}
              {eligible.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            {d.kind === "task" && !d.projectId && (
              <span className="text-[11px] text-danger">a task routine needs a project board</span>
            )}
          </div>
        )}

        <textarea
          value={d.prompt}
          onChange={(e) => set({ prompt: e.target.value })}
          rows={3}
          placeholder={
            d.kind === "task"
              ? "What should the card ask for? e.g. Update dependencies, run the tests, leave the diff for review."
              : d.kind === "research"
                ? "The question to investigate each time. e.g. What changed this week in the repos I depend on?"
                : d.kind === "watch"
                  ? "What to watch for. e.g. new openings, price changes, updated docs — each check reports what changed since the last."
                  : "The message to send each time. You can @mention agents and skills."
          }
          className={`${field} w-full resize-y`}
        />

        {/* The schedule builder. Presets compile to cron; custom is cron. */}
        <div className="flex flex-wrap items-center gap-2">
          <select value={preset} onChange={(e) => setPreset(e.target.value as Preset)} className={field}>
            <option value="hourly">Every hour</option>
            <option value="daily">Every day</option>
            <option value="weekdays">Weekdays</option>
            <option value="weekly">Every week</option>
            <option value="monthly">Every month</option>
            <option value="custom">Custom (cron)</option>
          </select>
          {preset === "weekly" && (
            <select value={weekday} onChange={(e) => setWeekday(parseInt(e.target.value, 10))} className={field}>
              {WEEKDAYS.map((w, i) => (
                <option key={w} value={i}>
                  {w}
                </option>
              ))}
            </select>
          )}
          {preset === "monthly" && (
            <select value={monthday} onChange={(e) => setMonthday(parseInt(e.target.value, 10))} className={field}>
              {Array.from({ length: 28 }, (_, i) => i + 1).map((n) => (
                <option key={n} value={n}>
                  day {n}
                </option>
              ))}
            </select>
          )}
          {preset !== "hourly" && preset !== "custom" && (
            <input type="time" value={time} onChange={(e) => setTime(e.target.value)} className={field} />
          )}
          {preset === "custom" && (
            <input
              value={custom}
              onChange={(e) => setCustom(e.target.value)}
              placeholder="m h dom mon dow"
              className={`${field} font-mono`}
            />
          )}
        </div>
        <div className="text-[11px] text-ink-dim">
          {preview === null && "…"}
          {preview?.valid === false && <span className="text-danger">That isn't a valid schedule.</span>}
          {preview?.valid && preview.next.length > 0 && (
            <>would next fire {preview.next.map((t) => new Date(t).toLocaleString()).join(", then ")}</>
          )}
        </div>

        <label className="flex items-center gap-2 text-xs text-ink-dim">
          <input
            type="checkbox"
            checked={(d.catchUp ?? "run_once") === "run_once"}
            onChange={(e) => set({ catchUp: e.target.checked ? "run_once" : "skip" })}
          />
          If my machine was asleep at the time, run once when it wakes
        </label>

        <div className="flex flex-wrap items-center gap-2">
          <EnginePicker
            value={d.engine ?? null}
            onChange={(engine) => set({ engine })}
            inheritLabel="Default engine"
          />
          <select
            value={d.modelTier ?? ""}
            onChange={(e) => set({ modelTier: e.target.value || null })}
            className={field}
            title="Which model tier each firing uses"
          >
            <option value="">Default model</option>
            <option value="easy">Easy</option>
            <option value="medium">Medium</option>
            <option value="complex">Complex</option>
          </select>
          <select
            value={d.effort ?? ""}
            onChange={(e) => set({ effort: e.target.value || null })}
            className={field}
            title="Reasoning effort"
          >
            <option value="">Default effort</option>
            <option value="low">Low</option>
            <option value="medium">Medium</option>
            <option value="high">High</option>
            <option value="xhigh">X-high</option>
            <option value="max">Max</option>
          </select>
        </div>

        <div className="flex items-center justify-end gap-2">
          <button onClick={onCancel} className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim">
            Cancel
          </button>
          <button
            onClick={() => onSave({ ...d, cronExpr })}
            disabled={
              !d.name.trim() ||
              !d.prompt.trim() ||
              preview?.valid === false ||
              (d.kind === "task" && !d.projectId) ||
              (d.kind === "watch" && !/^https?:\/\/\S{4,}$/.test((d.url ?? "").trim()))
            }
            className="rounded-lg bg-accent px-3.5 py-1.5 text-xs font-semibold text-white disabled:opacity-40"
          >
            {isNew ? "Create routine" : "Save changes"}
          </button>
        </div>
      </div>
    </div>
  );
}

function hostOf(url: string): string {
  try {
    return new URL(url).host;
  } catch {
    return url;
  }
}
