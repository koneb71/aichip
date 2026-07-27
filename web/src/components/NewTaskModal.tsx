import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Agent, api, Project, Team, Tier, tierColor, tierModel, tierSoft } from "../lib/api";
import { useWorkspace } from "../lib/workspace";

const TIERS: Tier[] = ["easy", "medium", "complex"];

export function NewTaskModal({
  project,
  onClose,
  onCreated,
}: {
  project: Project;
  onClose: () => void;
  onCreated: () => void;
}) {
  const { active } = useWorkspace();
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [tier, setTier] = useState<Tier>("medium");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [teams, setTeams] = useState<Team[]>([]);
  const [assignee, setAssignee] = useState<string>("");
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!active) return;
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
    api.teams(active.id).then((r) => setTeams(r.teams)).catch(() => {});
  }, [active]);

  // One picker, two kinds of assignee — a task goes to a person or a team,
  // never both.
  const [kind, id] = assignee ? assignee.split(":") : ["", ""];
  const assignedTeam = kind === "team" ? teams.find((t) => t.id === id) : undefined;

  const submit = async (start: boolean) => {
    if (!title.trim() || !prompt.trim() || busy) return;
    setBusy(true);
    try {
      await api.createTask({
        project_id: project.id,
        title: title.trim(),
        prompt: prompt.trim(),
        model_tier: tier,
        agent_id: kind === "agent" ? id : null,
        team_id: kind === "team" ? id : null,
        start,
      });
      onCreated();
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
        initial={{ y: 24, scale: 0.97 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 24, scale: 0.97 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-xl rounded-2xl border border-line bg-panel p-6"
      >
        <div className="mb-4 text-lg font-semibold">New task · {project.name}</div>
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Task title"
          className="mb-3 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <textarea
          value={prompt}
          onChange={(e) => setPrompt(e.target.value)}
          placeholder="Describe what the agent should do…"
          rows={5}
          className="mb-4 w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
        />

        <div className="mb-4 grid grid-cols-2 gap-4">
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
              Complexity → model
            </div>
            <div className="flex gap-1.5">
              {TIERS.map((t) => (
                <button
                  key={t}
                  onClick={() => setTier(t)}
                  className="flex-1 rounded-lg border px-2 py-1.5 text-xs capitalize"
                  style={{
                    borderColor: tier === t ? tierColor[t] : "var(--color-line)",
                    background: tier === t ? tierSoft[t] : "transparent",
                    color: tier === t ? tierColor[t] : "var(--color-ink-dim)",
                  }}
                >
                  {t}
                  <span className="block text-[10px] opacity-75">{tierModel[t]}</span>
                </button>
              ))}
            </div>
          </div>
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
              Assign to
            </div>
            <select
              value={assignee}
              onChange={(e) => setAssignee(e.target.value)}
              className="w-full rounded-lg border border-line bg-panel px-2 py-2 text-sm"
            >
              <option value="">Nobody in particular</option>
              {agents.length > 0 && (
                <optgroup label="Agents">
                  {agents.map((a) => (
                    <option key={a.id} value={`agent:${a.id}`}>
                      {a.name}
                    </option>
                  ))}
                </optgroup>
              )}
              {teams.length > 0 && (
                <optgroup label="Teams">
                  {teams.map((t) => (
                    <option key={t.id} value={`team:${t.id}`}>
                      {t.name} ({t.pattern})
                    </option>
                  ))}
                </optgroup>
              )}
            </select>
            {assignedTeam && (
              <div className="mt-1 text-[11px] text-ink-dim">
                {assignedTeam.pattern === "org"
                  ? "The manager will split this up and delegate it."
                  : `Runs as a ${assignedTeam.pattern}; the model tier above is ignored.`}
              </div>
            )}
          </div>
        </div>

        <div className="flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink">
            Cancel
          </button>
          <button
            onClick={() => submit(false)}
            disabled={busy}
            className="rounded-lg border border-line px-4 py-2 text-sm hover:bg-panel-2"
          >
            Add to backlog
          </button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => submit(true)}
            disabled={busy}
            className="rounded-lg bg-accent px-4 py-2 text-sm font-medium text-white hover:opacity-90"
          >
            Start now
          </motion.button>
        </div>
      </motion.div>
    </motion.div>
  );
}
