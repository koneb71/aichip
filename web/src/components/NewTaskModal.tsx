import { useEffect, useRef, useState } from "react";
import { Agent, api, Effort, Project, Skill, Team, TierChoice, tierColor, tierSoft } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { useAttachments } from "../lib/useAttachments";
import { AttachmentBar } from "./AttachmentBar";
import { useMentionPicker } from "./MentionPicker";
import { AssigneePicker, assigneeValue, parseAssignee } from "./AssigneePicker";
import { SkillPicker } from "./SkillPicker";
import { GoalPicker } from "./GoalPicker";
import { useTierModel } from "../lib/models";
import { EnginePicker, useEngines } from "../lib/engines";
import { ArticlePicker } from "./kb/ArticlePicker";
import { TIERS } from "./TierPicker";
import { EffortPicker } from "./EffortPicker";
import { estimateLine, ForecastAsk, parseForecastAsk } from "../lib/forecast";
import { Dialog } from "./ui/Dialog";
import { Button } from "./ui/Button";
import { Input, Textarea } from "./ui/Field";

export function NewTaskModal({
  project,
  onClose,
  onCreated,
}: {
  project: Project;
  onClose: () => void;
  onCreated: () => void;
}) {
  const tierModel = useTierModel();
  const engines = useEngines();
  const { active } = useWorkspace();
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [tier, setTier] = useState<TierChoice>("medium");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [teams, setTeams] = useState<Team[]>([]);
  const [skills, setSkills] = useState<Skill[]>([]);
  const [assignee, setAssignee] = useState<string>("");
  const [skillId, setSkillId] = useState<string | null>(null);
  const [goalId, setGoalId] = useState<string | null>(null);
  // null = the machine default, which is what the server picks.
  const [engine, setEngine] = useState<string | null>(null);
  // null = inherit: the agent's budget if it has one, else the machine default.
  const [effort, setEffort] = useState<Effort | null>(null);
  const [planFirst, setPlanFirst] = useState(false);
  const [articleIds, setArticleIds] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // What similar runs cost, and — when that could overrun a budget — the
  // question the server asked before starting the card it just made.
  const [estimate, setEstimate] = useState<string | null>(null);
  const [ask, setAsk] = useState<ForecastAsk | null>(null);
  const att = useAttachments(project.id);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const [caret, setCaret] = useState(0);
  const mention = useMentionPicker({
    projectId: project.id,
    // Files only. This form already has a picker for who does the work, and an
    // `@agent` typed into the prompt here would bind nothing — it would just be
    // a sentence the coding agent reads about itself.
    agents: [],
    text: prompt,
    caret,
    onApply: (text, nextCaret) => {
      setPrompt(text);
      setCaret(nextCaret);
      requestAnimationFrame(() => {
        promptRef.current?.setSelectionRange(nextCaret, nextCaret);
        promptRef.current?.focus();
      });
    },
  });

  // The dialog hears Escape before the textarea does (Radix listens on the
  // document, capturing), so which key closed it is noted on the way down.
  const escInPicker = useRef(false);
  const picking = mention.open;
  useEffect(() => {
    if (!picking) return;
    const onKey = (e: KeyboardEvent) => {
      escInPicker.current = e.key === "Escape" && document.activeElement === promptRef.current;
    };
    window.addEventListener("keydown", onKey, true);
    return () => {
      window.removeEventListener("keydown", onKey, true);
      escInPicker.current = false;
    };
  }, [picking]);

  useEffect(() => {
    if (!active) return;
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
    api.teams(active.id).then((r) => setTeams(r.teams)).catch(() => {});
    api.skills(active.id).then((r) => setSkills(r.skills)).catch(() => {});
  }, [active]);

  useEffect(() => {
    api
      .estimate(project.id, tier, engine ?? undefined)
      .then((r) => setEstimate(estimateLine(r.estimate)))
      .catch(() => setEstimate(null));
  }, [project.id, tier, engine]);

  // One picker, two kinds of assignee — a task goes to a person or a team,
  // never both.
  const [kind, id] = assignee ? assignee.split(":") : ["", ""];
  const assignedTeam = kind === "team" ? teams.find((t) => t.id === id) : undefined;

  const submit = async (start: boolean) => {
    // An attached spec with no prose is a reasonable task.
    if (!title.trim() || busy || att.busy) return;
    if (!prompt.trim() && att.ids.length === 0) return;
    setBusy(true);
    setError(null);
    try {
      await api.createTask({
        project_id: project.id,
        title: title.trim(),
        prompt: prompt.trim(),
        model_tier: tier,
        agent_id: kind === "agent" ? id : null,
        team_id: kind === "team" ? id : null,
        skill_id: skillId,
        goal_id: goalId,
        start,
        engine: engine ?? undefined,
        plan_first: planFirst,
        effort,
        article_ids: articleIds,
        attachment_ids: att.ids,
      });
      att.clear();
      onCreated();
    } catch (e) {
      // The card was made but not started: similar runs say it could
      // overrun a budget, so the person decides.
      const question = parseForecastAsk(String(e));
      if (question?.taskId) {
        att.clear();
        setAsk(question);
        return;
      }
      // Without this the modal swallowed every failure silently.
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (o) return;
        // Escape with the @ picker open dismisses the picker (its own key
        // handler does that), not the whole form. A click outside still closes.
        if (escInPicker.current) {
          escInPicker.current = false;
          return;
        }
        onClose();
      }}
      title={`New task · ${project.name}`}
      width={576}
      className={att.dragging ? "border-accent! ring-2 ring-accent/30" : undefined}
      footer={
        // What the buttons answer — a failure, or the budget question — sits
        // beside them, not somewhere up the scrolling form.
        <div className="flex w-full min-w-0 flex-col gap-2">
          {error && (
            <div className="rounded-lg bg-danger-subtle px-3 py-1.5 text-xs text-danger-fg">
              {error}
            </div>
          )}
          {ask && (
            <div className="rounded-lg bg-warning-subtle px-3 py-2 text-xs text-warning-fg">
              <div>
                The card is in the backlog. {ask.message.charAt(0).toUpperCase() + ask.message.slice(1)}.
              </div>
              <div className="mt-2 flex gap-2">
                <Button
                  variant="primary"
                  size="xs"
                  onClick={async () => {
                    try {
                      await api.startTask(ask.taskId!, true);
                      onCreated();
                    } catch (e) {
                      setAsk(null);
                      setError(String(e));
                    }
                  }}
                >
                  Start anyway
                </Button>
                <Button size="xs" onClick={onCreated} className="border-warning/40!">
                  Leave it in the backlog
                </Button>
              </div>
            </div>
          )}
          {estimate && !ask && <div className="text-right text-[11px] text-fg-muted">{estimate}</div>}
          <div className="flex justify-end gap-2">
            <Button variant="ghost" onClick={onClose}>
              Cancel
            </Button>
            <Button onClick={() => submit(false)} disabled={busy || ask !== null}>
              Add to backlog
            </Button>
            <Button variant="primary" onClick={() => submit(true)} disabled={busy || ask !== null}>
              {planFirst ? "Plan it" : "Start now"}
            </Button>
          </div>
        </div>
      }
    >
      {/* Drop anywhere in the modal, not just on the prompt box. */}
      <div {...att.dropProps}>
        <Input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Task title"
          className="mb-3"
        />
        <div className="relative mb-2">
          {mention.node}
          <Textarea
            ref={promptRef}
            value={prompt}
            onChange={(e) => {
              setPrompt(e.target.value);
              setCaret(e.target.selectionStart ?? 0);
            }}
            onSelect={(e) => setCaret(e.currentTarget.selectionStart ?? 0)}
            onPaste={att.onPaste}
            onKeyDown={(e) => {
              // Picker first, or Enter picks nothing and just adds a newline.
              if (mention.handleKey(e)) e.preventDefault();
            }}
            placeholder="Describe what the agent should do… (@ to reference a file)"
            rows={5}
            className="resize-none"
          />
        </div>
        <div className="mb-4">
          <AttachmentBar
            items={att.items}
            onAdd={att.add}
            onRemove={att.remove}
            full={att.full}
            disabled={busy}
          />
        </div>

        <div className="mb-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
              Complexity → model
            </div>
            <div className="flex gap-1.5">
              {/* Auto sits alongside the three rather than replacing a default:
                  it is a choice to hand the choice over, and it has to be made
                  deliberately. */}
              <button
                onClick={() => setTier("auto")}
                className="flex-1 rounded-lg border px-2 py-1.5 text-xs"
                style={{
                  borderColor: tier === "auto" ? "var(--color-accent)" : "var(--color-border)",
                  color: tier === "auto" ? "var(--color-accent)" : "var(--color-fg-muted)",
                }}
              >
                auto
                <span className="block text-[10px] opacity-75">picked per task</span>
              </button>
              {TIERS.map((t) => (
                <button
                  key={t}
                  onClick={() => setTier(t)}
                  className="flex-1 rounded-lg border px-2 py-1.5 text-xs capitalize"
                  style={{
                    borderColor: tier === t ? tierColor[t] : "var(--color-border)",
                    background: tier === t ? tierSoft[t] : "transparent",
                    color: tier === t ? tierColor[t] : "var(--color-fg-muted)",
                  }}
                >
                  {t}
                  <span className="block text-[10px] opacity-75">
                    {tierModel(t, engine ?? undefined)}
                  </span>
                </button>
              ))}
            </div>
          </div>
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
              Assign to
            </div>
            <AssigneePicker
              value={parseAssignee(assignee)}
              agents={agents}
              teams={teams}
              onChange={(next) => setAssignee(assigneeValue(next))}
            />
          </div>
        </div>

        {/* Only once there is something to pick. An empty control here would
            just be a question nobody in this workspace can answer yet. */}
        {skills.some((s) => s.enabled) && (
          <div className="mb-4">
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
              How
            </div>
            <SkillPicker value={skillId} skills={skills} onChange={setSkillId} />
          </div>
        )}

        {!!engines && engines.length > 1 && (
          <div className="mb-4">
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
              Run on
            </div>
            <EnginePicker value={engine} onChange={setEngine} inheritLabel="Default" />
          </div>
        )}

        <div className="mb-4">
          <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-fg-muted">
            Thinking
          </div>
          <EffortPicker value={effort} onChange={setEffort} />
        </div>

        {!!active && (
          <div className="mb-4">
            <GoalPicker workspaceId={active.id} value={goalId} onChange={setGoalId} />
          </div>
        )}

        {!!active && (
          <div className="mb-4">
            <ArticlePicker
              workspaceId={active.id}
              selected={articleIds}
              onChange={setArticleIds}
            />
          </div>
        )}

        <label className="mb-4 flex cursor-pointer items-start gap-2 text-sm">
          <input
            type="checkbox"
            checked={planFirst}
            onChange={(e) => setPlanFirst(e.target.checked)}
            className="mt-0.5 accent-[var(--color-accent)]"
          />
          <span className="min-w-0">
            <span className="block font-medium">Plan first</span>
            <span className="block text-xs text-fg-muted">
              The agent writes down what it intends to do and stops. You confirm
              it, rewrite it, or send it back — then work starts.
            </span>
          </span>
        </label>
      </div>
    </Dialog>
  );
}
