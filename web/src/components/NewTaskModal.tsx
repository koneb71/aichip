import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { Agent, api, Project, Team, Tier, tierColor, tierSoft } from "../lib/api";
import { useWorkspace } from "../lib/workspace";
import { useAttachments } from "../lib/useAttachments";
import { AttachmentBar } from "./AttachmentBar";
import { useMentionPicker } from "./MentionPicker";
import { AssigneePicker, assigneeValue, parseAssignee } from "./AssigneePicker";
import { useTierModel } from "../lib/models";

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
  const tierModel = useTierModel();
  const { active } = useWorkspace();
  const [title, setTitle] = useState("");
  const [prompt, setPrompt] = useState("");
  const [tier, setTier] = useState<Tier>("medium");
  const [agents, setAgents] = useState<Agent[]>([]);
  const [teams, setTeams] = useState<Team[]>([]);
  const [assignee, setAssignee] = useState<string>("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const att = useAttachments(project.id);
  const promptRef = useRef<HTMLTextAreaElement>(null);
  const [caret, setCaret] = useState(0);
  const mention = useMentionPicker({
    projectId: project.id,
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
        start,
        attachment_ids: att.ids,
      });
      att.clear();
      onCreated();
    } catch (e) {
      // Without this the modal swallowed every failure silently.
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4 sm:p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 24, scale: 0.97 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 24, scale: 0.97 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        // Drop anywhere in the modal, not just on the prompt box.
        {...att.dropProps}
        className={`card-shadow max-h-[90vh] w-full max-w-xl overflow-y-auto rounded-2xl border bg-panel p-5 sm:p-6 ${
          att.dragging ? "border-accent ring-2 ring-accent/30" : "border-line"
        }`}
      >
        <div className="mb-4 text-lg font-semibold">New task · {project.name}</div>
        <input
          autoFocus
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          placeholder="Task title"
          className="mb-3 w-full rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
        />
        <div className="relative mb-2">
          {mention.node}
          <textarea
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
            className="w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
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

        {error && (
          <div className="mb-3 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">
            {error}
          </div>
        )}

        <div className="mb-4 grid grid-cols-1 gap-4 sm:grid-cols-2">
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
                  <span className="block text-[10px] opacity-75">{tierModel(t)}</span>
                </button>
              ))}
            </div>
          </div>
          <div>
            <div className="mb-2 text-xs font-semibold uppercase tracking-wide text-ink-dim">
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
