import { HistoryButton } from "../components/RevisionsPanel";
import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, OrgRunSummary, Project, Team, TeamEstimate } from "../lib/api";
import { EnginePicker, ToolsNote, useEngines } from "../lib/engines";
import { useWorkspace } from "../lib/workspace";
import { formChanged } from "../lib/formDirty";
import { OrgRunView } from "../components/orgs/OrgRunView";
import { isWorking, needsYou, statusColor } from "../lib/runStatus";
import { Page, PageHead } from "../components/ui/Surface";
import { Button, IconButton } from "../components/ui/Button";
import { Dialog, Sheet } from "../components/ui/Dialog";
import { ArrowDown, ArrowUp, Plus, X } from "lucide-react";

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
    // All of them: a team that still names a retired agent shows who it was.
    api.allAgents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
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
    <Page>
      <PageHead
        title="Teams"
        subtitle="Compose agents into coordination patterns — or build an organization with a manager who plans the work and delegates it."
        actions={
          <Button size="sm" variant="primary" icon={<Plus className="size-3.5" />} onClick={() => setEditing("new")}>
            New team
          </Button>
        }
      />

      <div className="mt-6 grid max-w-4xl grid-cols-1 gap-4 sm:grid-cols-2">
        {teams.map((t) => (
          <motion.div
            key={t.id}
            layout
            whileHover={{ y: -2 }}
            className="card-shadow rounded-xl border border-border bg-panel p-4"
          >
            <div className="flex items-center justify-between">
              <div className="text-sm font-semibold">{t.name}</div>
              <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] capitalize text-fg-muted">
                {t.pattern}
              </span>
            </div>
            <div className="mt-3 flex items-center gap-2">
              {t.pattern === "org" && t.definition.manager && (
                <>
                  <span
                    title={`${agentById(t.definition.manager)?.name} — manager`}
                    className="flex h-8 w-8 items-center justify-center rounded-lg text-xs font-bold text-on-accent ring-2 ring-accent ring-offset-1 ring-offset-panel"
                    style={{ background: agentById(t.definition.manager)?.color ?? "var(--color-fg-subtle)" }}
                  >
                    {(agentById(t.definition.manager)?.name ?? "?").slice(0, 1).toUpperCase()}
                  </span>
                  <span className="text-fg-muted">→</span>
                </>
              )}
              <div className="flex -space-x-1.5">
                {(t.definition.members ?? []).map((m, i) => {
                  const a = agentById(m.agent_id);
                  return (
                    <span
                      key={i}
                      title={`${a?.name}${m.role ? ` — ${m.role}` : ""}`}
                      className="flex h-7 w-7 items-center justify-center rounded-full border-2 border-panel text-[11px] font-bold text-on-accent"
                      style={{ background: a?.color ?? "var(--color-fg-subtle)" }}
                    >
                      {(a?.name ?? "?").slice(0, 1).toUpperCase()}
                    </span>
                  );
                })}
                {(t.definition.members ?? []).length === 0 && (
                  <span className="text-xs text-fg-muted">No members yet</span>
                )}
              </div>
            </div>
            <div className="mt-4 flex gap-2">
              <Button
                variant="primary"
                size="sm"
                onClick={() => setRunning(t)}
                disabled={
                  (t.definition.members ?? []).length === 0 ||
                  (t.pattern === "org" && !t.definition.manager)
                }
              >
                {t.pattern === "org" ? "▶ Run organization" : "▶ Run team"}
              </Button>
              <Button size="sm" onClick={() => setEditing(t)}>
                Edit
              </Button>
            </div>
          </motion.div>
        ))}
        {teams.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-border p-8 text-center text-sm text-fg-muted">
            No teams yet — compose your agents into a pipeline, debate, or swarm.
          </div>
        )}
      </div>

      {orgRuns.length > 0 && (
        <>
          <h2 className="mt-10 text-sm font-semibold">Organization runs</h2>
          <div className="mt-3 flex max-w-4xl flex-col gap-1.5">
            {orgRuns.map((r) => {
              const live = isWorking(r.status);
              const color = statusColor(r.status);
              return (
                <motion.button
                  layout
                  key={r.id}
                  onClick={() => setOpenOrgRun(r.id)}
                  className="card-shadow flex items-center gap-3 rounded-xl border border-border bg-panel px-4 py-2.5 text-left"
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
                  <span className="min-w-0 flex-1 truncate text-xs text-fg-muted">
                    {r.goal}
                  </span>
                  {needsYou(r.status) && (
                    <span className="shrink-0 rounded-full bg-warning-subtle px-2 py-0.5 text-[11px] font-medium text-warning-fg">
                      needs you
                    </span>
                  )}
                  {r.costUsd != null && (
                    <span className="text-xs text-fg-muted">${r.costUsd.toFixed(3)}</span>
                  )}
                  <span className="text-xs text-fg-muted">
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
            agents={agents.filter((a) => a.status !== "retired")}
            onClose={() => setEditing(null)}
            onChanged={() => {
              setEditing(null);
              refresh();
            }}
          />
        )}
      </AnimatePresence>
    </Page>
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
  const [reviewPlan, setReviewPlan] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [estimate, setEstimate] = useState<TeamEstimate | null>(null);
  const navigate = useNavigate();

  // What this team has cost before. An org run can quietly become $15 and
  // forty minutes, and there was nothing here to suggest that beforehand.
  useEffect(() => {
    api.teamEstimate(team.id).then(setEstimate).catch(() => {});
  }, [team.id]);

  const start = async () => {
    if (!projectId || !goal.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (team.pattern === "org") {
        // Orgs get their own live view — that's the whole point of watching.
        const { runId } = await api.runOrg(team.id, projectId, goal.trim(), reviewPlan);
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
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      width={512}
      title={`Run ${team.name}`}
      description={
        <>
          The team's <span className="capitalize">{team.pattern}</span> pattern becomes a
          workflow, then runs step by step on your board.
        </>
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={start} disabled={busy || !goal.trim() || !projectId}>
            {busy ? "Starting…" : "Start run"}
          </Button>
        </>
      }
    >
      {projects.length === 0 ? (
        <div className="rounded-lg border border-dashed border-border p-4 text-center text-sm text-fg-muted">
          Load a project folder first.
        </div>
      ) : (
        <>
          <select
            value={projectId}
            onChange={(e) => setProjectId(e.target.value)}
            className="w-full rounded-lg border border-border bg-panel px-3 py-2 text-sm"
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
            className="mt-3 w-full resize-none rounded-lg border border-border bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
          />
          {team.pattern === "org" && (
            <label className="mt-3 flex cursor-pointer items-start gap-2 rounded-lg border border-border p-2.5">
              <input
                type="checkbox"
                checked={reviewPlan}
                onChange={(e) => setReviewPlan(e.target.checked)}
                className="mt-0.5 accent-[var(--color-accent)]"
              />
              <span className="text-xs">
                <span className="font-medium">Review the plan before work starts</span>
                <span className="mt-0.5 block text-fg-muted">
                  The team pauses after planning so you can reword, reassign, or drop
                  assignments. Cheaper than finding out an hour in.
                </span>
              </span>
            </label>
          )}
          {estimate && estimate.runs > 0 && estimate.medianUsd != null && (
            <div className="mt-3 flex items-center gap-2 rounded-lg bg-panel-2 px-3 py-2 text-xs text-fg-muted">
              <span>◷</span>
              <span>
                Past runs cost about{" "}
                <span className="font-semibold text-fg">
                  ${estimate.medianUsd.toFixed(2)}
                </span>
                {estimate.medianSecs != null &&
                  ` and took ${Math.round(estimate.medianSecs / 60)} min`}{" "}
                (median of {estimate.runs})
                {estimate.worstUsd != null &&
                  estimate.worstUsd > estimate.medianUsd * 1.5 &&
                  `; the worst was $${estimate.worstUsd.toFixed(2)}`}
                .
              </span>
            </div>
          )}
        </>
      )}

      {error && (
        <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
          {error}
        </div>
      )}
    </Dialog>
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
  // null = inherit from the card that summoned the team.
  const [engine, setEngine] = useState<string | null>(team?.engine ?? null);
  const engines = useEngines();
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
      engine,
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

  // Against what it opened with: member order is part of a team, so a
  // reorder counts as an edit.
  const dirty = formChanged(
    {
      name: team?.name,
      pattern: team?.pattern ?? "pipeline",
      members: team?.definition.members ?? [],
      manager: team?.definition.manager,
      engine: team?.engine,
    },
    { name, pattern, members, manager, engine },
  );

  const move = (index: number, dir: -1 | 1) =>
    setMembers((prev) => {
      const next = [...prev];
      const j = index + dir;
      if (j < 0 || j >= next.length) return prev;
      [next[index], next[j]] = [next[j], next[index]];
      return next;
    });

  return (
    <Sheet
      open
      onOpenChange={(o) => !o && onClose()}
      dismissible={!dirty}
      width={480}
      title={team ? `Edit ${team.name}` : "New team"}
      actions={team && <HistoryButton kind="team" id={team.id} onRestored={onClose} />}
    >
      <div className="flex h-full flex-col">
        <div className="min-h-0 flex-1 space-y-5 overflow-y-auto p-5">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            placeholder="Team name"
            className="w-full rounded-lg border border-border bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
          />

          {!!engines && engines.length > 1 && (
            <div className="flex items-center gap-2">
              <span className="text-xs font-semibold uppercase tracking-wide text-fg-muted">
                Run on
              </span>
              <EnginePicker
                value={engine}
                onChange={setEngine}
                inheritLabel="Whatever the card says"
              />
            </div>
          )}
          <ToolsNote engine={engine} what="a team" />

          <div className="grid grid-cols-2 gap-2">
            {PATTERNS.map((p) => (
              <button
                key={p.key}
                onClick={() => setPattern(p.key)}
                className={`rounded-xl border p-3 text-left ${
                  pattern === p.key ? "border-accent" : "border-border"
                }`}
              >
                <div className={`text-sm font-semibold ${pattern === p.key ? "text-accent-fg" : ""}`}>
                  {p.label}
                </div>
                <div className="mt-1 text-[11px] leading-snug text-fg-muted">{p.blurb}</div>
              </button>
            ))}
          </div>

          {pattern === "org" && (
            <div>
              <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
                Manager
              </div>
              <select
                value={manager}
                onChange={(e) => {
                  setManager(e.target.value);
                  setMembers((prev) => prev.filter((m) => m.agent_id !== e.target.value));
                }}
                className="w-full rounded-lg border border-border bg-panel px-3 py-2 text-sm"
              >
                <option value="">Pick who runs this team…</option>
                {agents.map((a) => (
                  <option key={a.id} value={a.id}>
                    {a.name}
                  </option>
                ))}
              </select>
              <div className="mt-1 text-[11px] text-fg-muted">
                Analyzes the goal, splits it into assignments, and answers questions while
                the team works. Pick someone strong — this one thinks, it doesn't code.
              </div>
            </div>
          )}

          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
              {pattern === "org" ? "Specialists" : "Members"}
              {pattern === "pipeline" && " (in order)"}
            </div>
            <div className="flex flex-col gap-1.5">
              {members.map((m, i) => {
                const a = agents.find((x) => x.id === m.agent_id);
                return (
                  <div key={m.agent_id} className="flex items-center gap-2 rounded-lg border border-border px-2 py-1.5">
                    <span
                      className="flex h-6 w-6 items-center justify-center rounded-full text-[11px] font-bold text-on-accent"
                      style={{ background: a?.color ?? "var(--color-fg-subtle)" }}
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
                        className="min-w-0 flex-1 rounded border border-border bg-panel px-1.5 py-0.5 text-xs outline-none focus:border-accent"
                      />
                    )}
                    {pattern !== "org" && <span className="flex-1" />}
                    <IconButton size="xs" label="Move up" onClick={() => move(i, -1)}>
                      <ArrowUp className="size-3.5" />
                    </IconButton>
                    <IconButton size="xs" label="Move down" onClick={() => move(i, 1)}>
                      <ArrowDown className="size-3.5" />
                    </IconButton>
                    <IconButton
                      size="xs"
                      label="Remove"
                      onClick={() => setMembers((prev) => prev.filter((x) => x.agent_id !== m.agent_id))}
                      className="hover:text-danger-fg!"
                    >
                      <X className="size-3.5" />
                    </IconButton>
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
                className="mt-2 w-full rounded-lg border border-dashed border-border bg-panel px-3 py-2 text-sm text-fg-muted"
              >
                <option value="">+ Add agent…</option>
                {available.map((a) => (
                  <option key={a.id} value={a.id}>{a.name}</option>
                ))}
              </select>
            )}
            {agents.length === 0 && (
              <div className="mt-2 text-xs text-fg-muted">
                Create some agents first — try “Generate with AI” on the Agents page.
              </div>
            )}
          </div>
          {error && <div className="rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">{error}</div>}
        </div>

        <div className="flex items-center justify-between border-t border-border p-4">
          {team ? (
            <Button
              variant="danger"
              size="sm"
              onClick={async () => {
                await api.deleteTeam(team.id);
                onChanged();
              }}
            >
              Delete
            </Button>
          ) : (
            <span />
          )}
          <Button variant="primary" onClick={save} disabled={busy || !name.trim()}>
            {busy ? "Saving…" : "Save team"}
          </Button>
        </div>
      </div>
    </Sheet>
  );
}
