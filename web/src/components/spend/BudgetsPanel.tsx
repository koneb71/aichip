import { HistoryButton } from "../RevisionsPanel";
import { useCallback, useEffect, useState } from "react";
import { api, BudgetBody, BudgetCap, BudgetScopeKind, BudgetStanding } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";
import { compactTokens } from "../../lib/spend";
import { RunError } from "../ui/RunError";

/**
 * What each scope may spend, and where it stands this window.
 *
 * Lives in the spend card's section because the numbers it governs are right
 * there — a cap set on a settings page elsewhere is a cap nobody sets. One
 * row per policy: a bar per cap, when it turns over, how many queued runs it
 * is holding, and — at this rate — when it runs out.
 */
export function BudgetsPanel({ onChanged }: { onChanged?: () => void }) {
  const [rows, setRows] = useState<BudgetStanding[] | null>(null);
  const [unpriced, setUnpriced] = useState<string[]>([]);
  const [editing, setEditing] = useState<BudgetStanding | "new" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .budgets()
      .then((r) => {
        setRows(r.policies);
        setUnpriced(r.unpricedEngines);
      })
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
  }, []);
  useEffect(load, [load]);

  const changed = () => {
    setEditing(null);
    load();
    onChanged?.();
  };

  return (
    <div className="card-shadow mt-4 rounded-xl border border-line bg-panel p-5">
      <div className="flex items-center justify-between">
        <div className="text-xs font-semibold uppercase tracking-wider text-ink-dim">Budgets</div>
        {!editing && (
          <button
            onClick={() => setEditing("new")}
            className="rounded-lg border border-line px-2.5 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
          >
            + Add a budget
          </button>
        )}
      </div>
      <p className="mt-1 text-[11px] text-ink-dim/80">
        A spent budget refuses new work at the click and holds what is queued until it resets. Dollars are
        only known when a run ends; a token cap set to stop can end a run midway.
      </p>

      {error && <RunError reason={error} className="mt-3" />}

      {editing && (
        <BudgetForm
          initial={editing === "new" ? null : editing}
          onCancel={() => setEditing(null)}
          onSaved={changed}
        />
      )}

      <div className="mt-3 flex flex-col gap-3">
        {rows?.length === 0 && !editing && (
          <div className="text-xs text-ink-dim">No budgets — nothing here limits what runs.</div>
        )}
        {rows?.map((row) => (
          <BudgetRow key={row.policy.id} row={row} unpriced={unpriced} onEdit={() => setEditing(row)} onChanged={changed} />
        ))}
      </div>
    </div>
  );
}

function BudgetRow({
  row,
  unpriced,
  onEdit,
  onChanged,
}: {
  row: BudgetStanding;
  unpriced: string[];
  onEdit: () => void;
  onChanged: () => void;
}) {
  const { policy: p, used, verdict } = row;
  const [overriding, setOverriding] = useState(false);
  const spent = verdict.state === "exceeded";
  const bars: { cap: BudgetCap; label: string; used: number; limit: number; fmt: (n: number) => string }[] = [];
  if (p.capUsd != null) bars.push({ cap: "usd", label: "Dollars", used: used.usd, limit: p.capUsd, fmt: (n) => `$${n.toFixed(2)}` });
  if (p.capOutputTokens != null)
    bars.push({ cap: "tokens", label: "Output tokens", used: used.outputTokens, limit: p.capOutputTokens, fmt: compactTokens });
  if (p.capRuns != null) bars.push({ cap: "runs", label: "Runs", used: used.runs, limit: p.capRuns, fmt: String });

  return (
    <div className={`rounded-lg border p-3 ${spent ? "border-amber-300 bg-amber-50/60" : "border-line"} ${p.enabled ? "" : "opacity-60"}`}>
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-sm font-medium">{p.name}</span>
        <span className="text-[11px] text-ink-dim">
          {row.scopeLabel} · per {p.windowKind} · {p.stopsInFlight ? "stops runs" : "holds new work"}
        </span>
        {!p.enabled && <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[10px] text-ink-dim">off</span>}
        {verdict.state === "warn" && (
          <span className="rounded-full bg-amber-50 px-2 py-0.5 text-[10px] text-amber-700">{verdict.percent}% spent</span>
        )}
        {spent && <span className="rounded-full bg-amber-100 px-2 py-0.5 text-[10px] font-medium text-amber-800">spent</span>}
        <div className="ml-auto flex gap-1.5">
          <button onClick={() => setOverriding((o) => !o)} className="rounded-md border border-line px-2 py-0.5 text-[11px] hover:bg-panel-2">
            Override
          </button>
          <button onClick={onEdit} className="rounded-md border border-line px-2 py-0.5 text-[11px] hover:bg-panel-2">
            Edit
          </button>
          <HistoryButton kind="budget_policy" id={row.policy.id} onRestored={onChanged} />
        </div>
      </div>

      <div className="mt-2 flex flex-col gap-1.5">
        {bars.map((b) => {
          const pct = Math.min(100, (b.used / Math.max(b.limit, 1e-9)) * 100);
          const over = verdict.state === "exceeded" && verdict.cap === b.cap;
          return (
            <div key={b.cap} className="flex items-center gap-3">
              <div className="w-28 shrink-0 text-[11px] text-ink-dim">{b.label}</div>
              <div className="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-panel-2">
                <div
                  style={{ width: `${pct}%` }}
                  className={`h-full rounded-full transition-[width] duration-500 ${over ? "bg-amber-500" : pct >= p.warnPercent ? "bg-amber-400" : "bg-accent"}`}
                />
              </div>
              <div className="w-36 shrink-0 text-right text-[11px] tabular-nums text-ink-dim">
                {b.fmt(b.used)} of {b.fmt(b.limit)}
              </div>
            </div>
          );
        })}
      </div>

      <div className="mt-1.5 flex flex-wrap gap-x-3 text-[11px] text-ink-dim">
        <span>Resets {new Date(row.windowEnd).toLocaleString()}</span>
        {!spent && row.forecast?.runsOutAt && (
          <span className="text-amber-700">At this rate it runs out {new Date(row.forecast.runsOutAt).toLocaleString()}</span>
        )}
        {row.held > 0 && <span className="text-amber-700">{row.held} queued {row.held === 1 ? "run" : "runs"} held</span>}
      </div>

      {/* Said where it matters: a dollar cap is blind to an engine that never
          reports a price, and only a token cap counts that work. */}
      {p.capUsd != null && p.capOutputTokens == null && unpriced.length > 0 && (
        <div className="mt-1.5 text-[11px] text-ink-dim">
          {unpriced.join(" and ")} {unpriced.length === 1 ? "runs report" : "run report"} no price, so this dollar cap
          cannot see {unpriced.length === 1 ? "it" : "them"} — add a token cap to count {unpriced.length === 1 ? "it" : "them"}.
        </div>
      )}

      {overriding && <OverrideForm row={row} onDone={() => { setOverriding(false); onChanged(); }} />}
    </div>
  );
}

function OverrideForm({ row, onDone }: { row: BudgetStanding; onDone: () => void }) {
  const p = row.policy;
  const [amount, setAmount] = useState("");
  const [note, setNote] = useState("");
  const [error, setError] = useState<string | null>(null);
  // The cap most in the way, or the first one the policy has.
  const cap: BudgetCap =
    row.verdict.state === "exceeded" ? row.verdict.cap : p.capUsd != null ? "usd" : p.capOutputTokens != null ? "tokens" : "runs";
  const unit = cap === "usd" ? "dollars" : cap === "tokens" ? "output tokens" : "runs";

  const save = async () => {
    const n = Number(amount);
    if (!Number.isFinite(n) || n <= 0) return setError(`say how many more ${unit}`);
    try {
      await api.overrideBudget(p.id, {
        ...(cap === "usd" ? { usd: n } : cap === "tokens" ? { tokens: Math.round(n) } : { runs: Math.round(n) }),
        note,
      });
      onDone();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  return (
    <div className="mt-2 rounded-md bg-panel-2 p-2 text-xs">
      <div className="text-[11px] text-ink-dim">More room for this {p.windowKind} only — next {p.windowKind} it is the same budget again.</div>
      <div className="mt-1.5 flex flex-wrap items-center gap-1.5">
        <input
          autoFocus
          value={amount}
          onChange={(e) => setAmount(e.target.value)}
          inputMode="decimal"
          placeholder={cap === "usd" ? "10" : cap === "tokens" ? "50000" : "3"}
          className="w-24 rounded-md border border-line bg-panel px-2 py-1"
        />
        <span className="text-ink-dim">more {unit}, because</span>
        <input
          value={note}
          onChange={(e) => setNote(e.target.value)}
          placeholder="release day"
          className="min-w-0 flex-1 rounded-md border border-line bg-panel px-2 py-1"
        />
        <button onClick={save} className="rounded-md bg-accent px-2.5 py-1 text-[11px] font-medium text-white">
          Override
        </button>
      </div>
      {error && <RunError reason={error} className="mt-1.5" />}
    </div>
  );
}

type Option = { id: string; name: string };

function BudgetForm({
  initial,
  onCancel,
  onSaved,
}: {
  initial: BudgetStanding | null;
  onCancel: () => void;
  onSaved: () => void;
}) {
  const { active } = useWorkspace();
  const p = initial?.policy;
  const [name, setName] = useState(p?.name ?? "");
  const [scopeKind, setScopeKind] = useState<BudgetScopeKind>(p?.scopeKind ?? "project");
  const [scopeId, setScopeId] = useState<string>(p?.scopeId ?? "");
  const [windowKind, setWindowKind] = useState<BudgetBody["window_kind"]>(p?.windowKind ?? "week");
  const [usd, setUsd] = useState(p?.capUsd != null ? String(p.capUsd) : "");
  const [tokens, setTokens] = useState(p?.capOutputTokens != null ? String(p.capOutputTokens) : "");
  const [runs, setRuns] = useState(p?.capRuns != null ? String(p.capRuns) : "");
  const [warn, setWarn] = useState(String(p?.warnPercent ?? 80));
  const [stop, setStop] = useState(p?.stopsInFlight ?? false);
  const [confirmAbove, setConfirmAbove] = useState(p?.confirmAboveUsd != null ? String(p.confirmAboveUsd) : "");
  const [enabled, setEnabled] = useState(p?.enabled ?? true);
  const [options, setOptions] = useState<Option[]>([]);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!active || scopeKind === "machine") return setOptions([]);
    const ws = active.id;
    const load: Promise<Option[]> =
      scopeKind === "workspace"
        ? api.workspaces().then((r) => r.workspaces)
        : scopeKind === "project"
          ? // Apps are projects too, and generating one costs real money.
            Promise.all([api.projects(ws), api.apps(ws)]).then(([p, a]) => [
              ...p.projects,
              ...a.apps.map((app) => ({ id: app.projectId, name: `${app.name} (app)` })),
            ])
          : scopeKind === "agent"
            ? api.agents(ws).then((r) => r.agents)
            : scopeKind === "team"
              ? api.teams(ws).then((r) => r.teams)
              : api.routines(ws).then((r) => r.routines);
    load.then((o) => setOptions(o.map((x) => ({ id: x.id, name: x.name })))).catch(() => setOptions([]));
  }, [active, scopeKind]);

  /** Empty is "not set"; anything else must be a number, or the save says so
   *  — a typo must never quietly remove a cap. */
  const num = (s: string, what: string): number | null => {
    if (s.trim() === "") return null;
    const n = Number(s.trim().replace(/^\$/, ""));
    if (!Number.isFinite(n)) throw new Error(`${what}: \u201c${s}\u201d is not a number`);
    return n;
  };

  const save = async () => {
    setError(null);
    let body: BudgetBody;
    try {
      const warnPercent = num(warn, "Warn at") ?? 80;
      if (warnPercent < 1 || warnPercent > 100) throw new Error("Warn at: between 1 and 100");
      const tokenCap = num(tokens, "Output tokens");
      const runCap = num(runs, "Runs");
      body = {
      name,
      scope_kind: scopeKind,
      scope_id: scopeKind === "machine" ? null : scopeId || null,
      window_kind: windowKind,
      cap_usd: num(usd, "Dollars"),
      cap_output_tokens: tokenCap == null ? null : Math.round(tokenCap),
      cap_runs: runCap == null ? null : Math.round(runCap),
      warn_percent: Math.round(warnPercent),
      on_exceed: stop ? "stop" : "hold",
      confirm_above_usd: num(confirmAbove, "Ask above"),
      enabled,
      };
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      return;
    }
    try {
      if (p) await api.updateBudget(p.id, body);
      else await api.createBudget(body);
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const remove = async () => {
    if (!p) return;
    try {
      await api.deleteBudget(p.id);
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const field = "rounded-md border border-line bg-panel px-2 py-1 text-xs";
  return (
    <div className="mt-3 rounded-lg border border-accent/40 p-3 text-xs">
      <div className="grid grid-cols-1 gap-2 sm:grid-cols-2">
        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Name</span>
          <input value={name} onChange={(e) => setName(e.target.value)} placeholder="Nightly work" className={field} />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Covers</span>
          <div className="flex gap-1.5">
            <select
              value={scopeKind}
              onChange={(e) => {
                setScopeKind(e.target.value as BudgetScopeKind);
                setScopeId("");
              }}
              className={field}
            >
              <option value="machine">This machine</option>
              <option value="workspace">A workspace</option>
              <option value="project">A project</option>
              <option value="agent">An agent</option>
              <option value="team">A team</option>
              <option value="routine">A routine</option>
            </select>
            {scopeKind !== "machine" && (
              <select value={scopeId} onChange={(e) => setScopeId(e.target.value)} className={`${field} min-w-0 flex-1`}>
                <option value="">Which one?</option>
                {options.map((o) => (
                  <option key={o.id} value={o.id}>
                    {o.name}
                  </option>
                ))}
              </select>
            )}
          </div>
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Resets every</span>
          <select value={windowKind} onChange={(e) => setWindowKind(e.target.value as BudgetBody["window_kind"])} className={field}>
            <option value="day">day</option>
            <option value="week">week</option>
            <option value="month">month</option>
          </select>
        </label>
        <div className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Caps — any of them</span>
          <div className="flex gap-1.5">
            <input value={usd} onChange={(e) => setUsd(e.target.value)} inputMode="decimal" placeholder="$" className={`${field} w-20`} />
            <input value={tokens} onChange={(e) => setTokens(e.target.value)} inputMode="numeric" placeholder="output tokens" className={`${field} w-28`} />
            <input value={runs} onChange={(e) => setRuns(e.target.value)} inputMode="numeric" placeholder="runs" className={`${field} w-16`} />
          </div>
        </div>
        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Warn at (% spent)</span>
          <input value={warn} onChange={(e) => setWarn(e.target.value)} inputMode="numeric" className={`${field} w-20`} />
        </label>
        <label className="flex flex-col gap-1">
          <span className="text-[11px] text-ink-dim">Ask before starting work that could cost more than ($ of what is left)</span>
          <input value={confirmAbove} onChange={(e) => setConfirmAbove(e.target.value)} inputMode="decimal" placeholder="never" className={`${field} w-24`} />
        </label>
      </div>
      <label className="mt-2 flex cursor-pointer items-center gap-2 text-[11px] text-ink-dim">
        <input type="checkbox" checked={stop} onChange={(e) => setStop(e.target.checked)} className="accent-accent" />
        Also stop a run in flight when it crosses the token cap (otherwise only new work is held)
      </label>
      {stop && (
        <p className="mt-1 pl-6 text-[11px] text-ink-dim">
          Each run is measured against what was left when it started, so runs going at the same time can
          together pass the cap by up to their combined size.
        </p>
      )}
      <label className="mt-1 flex cursor-pointer items-center gap-2 text-[11px] text-ink-dim">
        <input type="checkbox" checked={enabled} onChange={(e) => setEnabled(e.target.checked)} className="accent-accent" />
        On
      </label>
      {error && <RunError reason={error} className="mt-2" />}
      <div className="mt-3 flex gap-2">
        <button onClick={save} className="rounded-md bg-accent px-3 py-1 text-[11px] font-medium text-white">
          {p ? "Save" : "Add budget"}
        </button>
        <button onClick={onCancel} className="rounded-md border border-line px-3 py-1 text-[11px] hover:bg-panel-2">
          Cancel
        </button>
        {p && (
          <button onClick={remove} className="ml-auto rounded-md px-2 py-1 text-[11px] text-ink-dim hover:text-danger">
            Delete
          </button>
        )}
      </div>
    </div>
  );
}
