import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, Team } from "../lib/api";
import { useWorkspace } from "../lib/workspace";

const PATTERNS: { key: Team["pattern"]; label: string; blurb: string }[] = [
  { key: "pipeline", label: "Pipeline", blurb: "Roles run in sequence — plan → build → review" },
  { key: "debate", label: "Debate", blurb: "Several solvers attempt in parallel; a judge picks" },
  { key: "swarm", label: "Swarm", blurb: "A lead splits work across parallel agents" },
];

export default function TeamsPage() {
  const { active } = useWorkspace();
  const [teams, setTeams] = useState<Team[]>([]);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [editing, setEditing] = useState<Team | "new" | null>(null);

  const refresh = useCallback(() => {
    if (!active) return;
    api.teams(active.id).then((r) => setTeams(r.teams)).catch(() => {});
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
  }, [active]);

  useEffect(refresh, [refresh]);

  const agentById = (id: string) => agents.find((a) => a.id === id);

  return (
    <div className="h-full overflow-y-auto p-8">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-xl font-bold tracking-tight">Teams</h1>
          <p className="mt-0.5 text-sm text-ink-dim">
            Compose agents into coordination patterns. Team execution ships with pipelines.
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
          <motion.button
            key={t.id}
            layout
            whileHover={{ y: -2 }}
            onClick={() => setEditing(t)}
            className="card-shadow rounded-xl border border-line bg-panel p-4 text-left"
          >
            <div className="flex items-center justify-between">
              <div className="text-sm font-semibold">{t.name}</div>
              <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] capitalize text-ink-dim">
                {t.pattern}
              </span>
            </div>
            <div className="mt-3 flex -space-x-1.5">
              {(t.definition.members ?? []).map((m, i) => {
                const a = agentById(m.agent_id);
                return (
                  <span
                    key={i}
                    title={a?.name}
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
          </motion.button>
        ))}
        {teams.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
            No teams yet — compose your agents into a pipeline, debate, or swarm.
          </div>
        )}
      </div>

      <AnimatePresence>
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
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const available = agents.filter((a) => !members.some((m) => m.agent_id === a.id));

  const save = async () => {
    if (!name.trim() || busy) return;
    setBusy(true);
    setError(null);
    const body = {
      workspace_id: workspaceId,
      name: name.trim(),
      pattern,
      definition: { members },
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

        <div className="grid grid-cols-3 gap-2">
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

        <div>
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
            Members {pattern === "pipeline" && "(in order)"}
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
                  <span className="min-w-0 flex-1 truncate text-sm">{a?.name ?? "Unknown"}</span>
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
