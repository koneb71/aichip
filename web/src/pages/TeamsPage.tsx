import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, OrgRunSummary, Project, Team } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { OrgRunView } from "../components/orgs/OrgRunView";

const PATTERNS: { key: Team["pattern"]; label: string; blurb: string }[] = [
  {
    key: "org",
    label: "Organization",
    blurb: "A manager reads the goal, splits it up, and delegates to specialists",
  },
  { key: "pipeline", label: "Pipeline", blurb: "Roles run in sequence — plan → build → review" },
  { key: "debate", label: "Debate", blurb: "Several solvers attempt in parallel; a judge picks" },
  { key: "swarm", label: "Swarm", blurb: "Everyone works the same goal in parallel" },
];

export default function TeamsPage() {
  const { active } = useWorkspace();
  const [teams, setTeams] = useState<Team[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [projects, setProjects] = useState<Project[]>([]);
  const [editing, setEditing] = useState<Team | "new" | null>(null);
  const [running, setRunning] = useState<Team | null>(null);
  const [openOrgRun, setOpenOrgRun] = useState<string | null>(null);

  const [orgRuns, setOrgRuns] = useState<OrgRunSummary[]>([]);

  const refresh = useCallback(() => {
    if (!active) return;
    api.teams(active.id).then((r) => setTeams(r.teams)).catch(() => {});
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
    api.projects(active.id).then((r) => setProjects(r.projects)).catch(() => {});
    api
      .orgRuns({ workspaceId: active.id })
      .then((r) => setOrgRuns(r.runs))
      .catch(() => {});
  }, [active]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 4000);
    return () => clearInterval(interval);
  }, [refresh]);

  const agentById = (id: string) => agents.find((a) => a.id === id);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Teams</h1>
          <p className="mt-0.5 text-sm text-ink-dim">
            Compose agents into coordination patterns — or build an organization with a
            manager who plans the work and delegates it.
          </p>
        </div>
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={() => setEditing("new")}
          className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white"
        >
          + New team
        </motion.button>
      </div>

      <div className="mt-6 grid max-w-4xl grid-cols-2 gap-4">
        {teams.map((t) => (
          <motion.div
            key={t.id}
            layout
            whileHover={{ y: -2 }}
            className="card-shadow rounded-xl border border-line bg-panel p-4"
          >
            <div className="flex items-center justify-between">
              <div className="text-sm font-semibold">{t.name}</div>
              <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] capitalize text-ink-dim">
                {t.pattern}
              </span>
            </div>
            <div className="mt-3 flex items-center gap-2">
              {t.pattern === "org" && t.definition.manager && (
                <>
                  <span
                    title={`${agentById(t.definition.manager)?.name} — manager`}
                    className="flex h-8 w-8 items-center justify-center rounded-lg text-xs font-bold text-white ring-2 ring-accent ring-offset-1"
                    style={{ background: agentById(t.definition.manager)?.color ?? "#9ca3af" }}
                  >
                    {(agentById(t.definition.manager)?.name ?? "?").slice(0, 1).toUpperCase()}
                  </span>
                  <span className="text-ink-dim">→</span>
                </>
              )}
              <div className="flex -space-x-1.5">
                {(t.definition.members ?? []).map((m, i) => {
                  const a = agentById(m.agent_id);
                  return (
                    <span
                      key={i}
                      title={`${a?.name}${m.role ? ` — ${m.role}` : ""}`}
                      className="flex h-7 w-7 items-center justify-center rounded-full border-2 border-panel text-[11px] font-bold text-white"
                      style={{ background: a?.color ?? "#9ca3af" }}
                    >
                      {(a?.name ?? "?").slice(0, 1).toUpperCase()}
                    </span>
                  );
                })}
                {(t.definition.members ?? []).length === 0 && (
                  <span className="text-xs text-ink-dim">No members yet</span>
                )}
              </div>
            </div>
            <div className="mt-4 flex gap-2">
              <motion.button
                whileTap={{ scale: 0.96 }}
                onClick={() => setRunning(t)}
                disabled={
                  (t.definition.members ?? []).length === 0 ||
                  (t.pattern === "org" && !t.definition.manager)
                }
                className="rounded-lg bg-accent px-3 py-1 text-xs font-medium text-white disabled:opacity-40"
              >
                {t.pattern === "org" ? "▶ Run organization" : "▶ Run team"}
              </motion.button>
              <button
                onClick={() => setEditing(t)}
                className="rounded-lg border border-line px-3 py-1 text-xs hover:bg-panel-2"
              >
                Edit
              </button>
            </div>
          </motion.div>
        ))}
        {teams.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
            No teams yet — compose your agents into a pipeline, debate, or swarm.
          </div>
        )}
      </div>

      {orgRuns.length > 0 && (
        <>
          <h2 className="mt-10 text-sm font-semibold">Organization runs</h2>
          <div className="mt-3 flex max-w-4xl flex-col gap-1.5">
            {orgRuns.map((r) => {
              const live =
                r.status === "running" || r.status === "starting" || r.status === "queued";
              const color =
                r.status === "completed"
                  ? "var(--color-tier-easy)"
                  : r.status === "failed"
                    ? "var(--color-danger)"
                    : "var(--color-tier-medium)";
              return (
                <motion.button
                  layout
                  key={r.id}
                  onClick={() => setOpenOrgRun(r.id)}
                  className="card-shadow flex items-center gap-3 rounded-xl border border-line bg-panel px-4 py-2.5 text-left"
                >
                  {live ? (
                    <motion.span
                      className="h-2.5 w-2.5 shrink-0 rounded-full"
                      style={{ background: color }}
                      animate={{ opacity: [1, 0.3, 1] }}
                      transition={{ repeat: Infinity, duration: 1.5 }}
                    />
                  ) : (
                    <span
                      className="h-2.5 w-2.5 shrink-0 rounded-full"
                      style={{ background: color }}
                    />
                  )}
                  <span className="shrink-0 text-sm font-medium">{r.teamName}</span>
                  <span className="min-w-0 flex-1 truncate text-xs text-ink-dim">
                    {r.goal}
                  </span>
                  {r.costUsd != null && (
                    <span className="text-xs text-ink-dim">${r.costUsd.toFixed(3)}</span>
                  )}
                  <span className="text-xs text-ink-dim">
                    {new Date(r.createdAt).toLocaleTimeString()}
                  </span>
                </motion.button>
              );
            })}
          </div>
        </>
      )}

      <AnimatePresence>
        {running && (
          <RunTeamModal
            team={running}
            projects={projects}
            onClose={() => setRunning(null)}
            onOrgStarted={(runId) => {
              setRunning(null);
              setOpenOrgRun(runId);
            }}
          />
        )}
        {openOrgRun && (
          <OrgRunView runId={openOrgRun} onClose={() => setOpenOrgRun(null)} />
        )}
        {editing && active && (
          <TeamEditor
            workspaceId={active.id}
            team={editing === "new" ? null : editing}
            agents={agents}
            onClose={() => setEditing(null)}
            onChanged={() => {
              setEditing(null);
              refresh();
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

function RunTeamModal({
  team,
  projects,
  onClose,
  onOrgStarted,
}: {
  team: Team;
  projects: Project[];
  onClose: () => void;
  onOrgStarted: (runId: string) => void;
}) {
  const [projectId, setProjectId] = useState(projects[0]?.id ?? "");
  const [goal, setGoal] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const navigate = useNavigate();

  const start = async () => {
    if (!projectId || !goal.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (team.pattern === "org") {
        // Orgs get their own live view — that's the whole point of watching.
        const { runId } = await api.runOrg(team.id, projectId, goal.trim());
        onOrgStarted(runId);
        return;
      }
      await api.runTeam(team.id, projectId, goal.trim());
      navigate(`/projects/${projectId}`);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-lg rounded-2xl border border-line bg-panel p-6"
      >
        <div className="text-base font-semibold">Run {team.name}</div>
        <div className="mt-0.5 text-xs text-ink-dim">
          The team's <span className="capitalize">{team.pattern}</span> pattern becomes a
          workflow, then runs step by step on your board.
        </div>

        {projects.length === 0 ? (
          <div className="mt-4 rounded-lg border border-dashed border-line p-4 text-center text-sm text-ink-dim">
            Load a project folder first.
          </div>
        ) : (
          <>
            <select
              value={projectId}
              onChange={(e) => setProjectId(e.target.value)}
              className="mt-4 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm"
            >
              {projects.map((p) => (
                <option key={p.id} value={p.id}>
                  {p.name}
                </option>
              ))}
            </select>
            <textarea
              autoFocus
              value={goal}
              onChange={(e) => setGoal(e.target.value)}
              rows={4}
              placeholder="What should the team accomplish?"
              className="mt-3 w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
            />
          </>
        )}

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink">
            Cancel
          </button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={start}
            disabled={busy || !goal.trim() || !projectId}
            className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {busy ? "Starting…" : "Start run"}
          </motion.button>
        </div>
      </motion.div>
    </motion.div>
  );
}

function TeamEditor({
  workspaceId,
  team,
  agents,
  onClose,
  onChanged,
}: {
  workspaceId: string;
  team: Team | null;
  agents: Agent[];
  onClose: () => void;
  onChanged: () => void;
}) {
  const [name, setName] = useState(team?.name ?? "");
  const [pattern, setPattern] = useState<Team["pattern"]>(team?.pattern ?? "pipeline");
  const [members, setMembers] = useState<{ agent_id: string; role?: string }[]>(
    team?.definition.members ?? [],
  );
  const [manager, setManager] = useState<string>(team?.definition.manager ?? "");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The manager delegates rather than doing the work, so keep them out of
  // the specialist list.
  const available = agents.filter(
    (a) => !members.some((m) => m.agent_id === a.id) && a.id !== manager,
  );

  const save = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    const body = {
      workspace_id: workspaceId,
      name: name.trim(),
      pattern,
      definition:
        pattern === "org" ? { manager: manager || undefined, members } : { members },
    };
    try {
      if (team) await api.updateTeam(team.id, body);
      else await api.createTeam(body);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const move = (index: number, dir: -1 | 1) =>
    setMembers((prev) => {
      const next = [...prev];
      const j = index + dir;
      if (j < 0 || j >= next.length) return prev;
      [next[index], next[j]] = [next[j], next[index]];
      return next;
    });

  return (
    <motion.aside
      initial={{ x: 480 }}
      animate={{ x: 0 }}
      exit={{ x: 480 }}
      transition={{ type: "spring", stiffness: 320, damping: 34 }}
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-[480px] flex-col border-l border-line bg-panel"
    >
      <div className="flex items-center justify-between border-b border-line p-5">
        <div className="text-base font-semibold">{team ? `Edit ${team.name}` : "New team"}</div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">✕</button>
      </div>

      <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-5">
        <input
          value={name}
          onChange={(e) => setName(e.target.value)}
          placeholder="Team name"
          className="w-full rounded-lg border border-line px-3 py-2 text-sm outline-none focus:border-accent"
        />

        <div className="grid grid-cols-2 gap-2">
          {PATTERNS.map((p) => (
            <button
              key={p.key}
              onClick={() => setPattern(p.key)}
              className={`rounded-xl border p-3 text-left ${
                pattern === p.key ? "border-accent" : "border-line"
              }`}
            >
              <div className={`text-sm font-semibold ${pattern === p.key ? "text-accent" : ""}`}>
                {p.label}
              </div>
              <div className="mt-1 text-[11px] leading-snug text-ink-dim">{p.blurb}</div>
            </button>
          ))}
        </div>

        {pattern === "org" && (
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
              Manager
            </div>
            <select
              value={manager}
              onChange={(e) => {
                setManager(e.target.value);
                setMembers((prev) => prev.filter((m) => m.agent_id !== e.target.value));
              }}
              className="w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm"
            >
              <option value="">Pick who runs this team…</option>
              {agents.map((a) => (
                <option key={a.id} value={a.id}>
                  {a.name}
                </option>
              ))}
            </select>
            <div className="mt-1 text-[11px] text-ink-dim">
              Analyzes the goal, splits it into assignments, and answers questions while
              the team works. Pick someone strong — this one thinks, it doesn't code.
            </div>
          </div>
        )}

        <div>
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
            {pattern === "org" ? "Specialists" : "Members"}
            {pattern === "pipeline" && " (in order)"}
          </div>
          <div className="flex flex-col gap-1.5">
            {members.map((m, i) => {
              const a = agents.find((x) => x.id === m.agent_id);
              return (
                <div key={m.agent_id} className="flex items-center gap-2 rounded-lg border border-line px-2 py-1.5">
                  <span
                    className="flex h-6 w-6 items-center justify-center rounded-full text-[11px] font-bold text-white"
                    style={{ background: a?.color ?? "#9ca3af" }}
                  >
                    {(a?.name ?? "?").slice(0, 1).toUpperCase()}
                  </span>
                  <span className="min-w-0 shrink-0 truncate text-sm">{a?.name ?? "Unknown"}</span>
                  {pattern === "org" && (
                    <input
                      value={m.role ?? ""}
                      onChange={(e) =>
                        setMembers((prev) =>
                          prev.map((x, j) => (j === i ? { ...x, role: e.target.value } : x)),
                        )
                      }
                      placeholder="their role on this team"
                      className="min-w-0 flex-1 rounded border border-line px-1.5 py-0.5 text-xs outline-none focus:border-accent"
                    />
                  )}
                  {pattern !== "org" && <span className="flex-1" />}
                  <button onClick={() => move(i, -1)} className="px-1 text-ink-dim hover:text-ink">↑</button>
                  <button onClick={() => move(i, 1)} className="px-1 text-ink-dim hover:text-ink">↓</button>
                  <button
                    onClick={() => setMembers((prev) => prev.filter((x) => x.agent_id !== m.agent_id))}
                    className="px-1 text-ink-dim hover:text-danger"
                  >
                    ✕
                  </button>
                </div>
              );
            })}
          </div>
          {available.length > 0 && (
            <select
              value=""
              onChange={(e) =>
                e.target.value &&
                setMembers((prev) => [...prev, { agent_id: e.target.value }])
              }
              className="mt-2 w-full rounded-lg border border-dashed border-line px-3 py-2 text-sm text-ink-dim"
            >
              <option value="">+ Add agent…</option>
              {available.map((a) => (
                <option key={a.id} value={a.id}>{a.name}</option>
              ))}
            </select>
          )}
          {agents.length === 0 && (
            <div className="mt-2 text-xs text-ink-dim">
              Create some agents first — try “Generate with AI” on the Agents page.
            </div>
          )}
        </div>
        {error && <div className="rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>}
      </div>

      <div className="flex items-center justify-between border-t border-line p-4">
        {team ? (
          <button
            onClick={async () => {
              await api.deleteTeam(team.id);
              onChanged();
            }}
            className="text-sm text-danger hover:underline"
          >
            Delete
          </button>
        ) : (
          <span />
        )}
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={save}
          disabled={busy || !name.trim()}
          className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
        >
          {busy ? "Saving…" : "Save team"}
        </motion.button>
      </div>
    </motion.aside>
  );
}
