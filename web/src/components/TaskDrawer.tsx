import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, Attachment, displayTier, PendingPermission, Task, Team, tierColor } from "../lib/api";
import { useRunStream, StreamEvent } from "../lib/ws";
import { isActive, isWorking, statusLabel } from "../lib/runStatus";
import { useAttachments } from "../lib/useAttachments";
import { AttachmentBar, AttachmentList } from "./AttachmentBar";
import { TaskComments } from "./TaskComments";
import { Markdown } from "./Markdown";
import { annotateDiff, hunkText, isCommentable } from "../lib/diff";
import { PermissionRow } from "./PermissionRow";
import { BakeoffView } from "./BakeoffView";
import { RunStream, ActivityLine } from "./RunStream";
import { AssigneePicker } from "./AssigneePicker";
import { PlanReviewPanel } from "./PlanReviewPanel";
import { ArticlePicker } from "./kb/ArticlePicker";
import { useTierModel } from "../lib/models";
import { EnginePicker, useEngines } from "../lib/engines";
import { PreviewPanel } from "./PreviewPanel";
import { CardTierPicker } from "./TierPicker";
import { EffortPicker } from "./EffortPicker";
import { PullRequestPanel } from "./PullRequestPanel";

export function TaskDrawer({
  onOpenPreviews,
  task,
  workspaceId,
  onClose,
  onChanged,
  onOpenTeamRoom,
  boardTasks = [],
  onOpenTask,
}: {
  task: Task;
  /** Bounds which agents a bake-off may choose between. */
  workspaceId: string;
  onClose: () => void;
  onChanged: () => void;
  onOpenTeamRoom?: (runId: string) => void;
  /** The project's cards, so an epic can list its own without a second fetch. */
  boardTasks?: Task[];
  onOpenTask?: (t: Task) => void;
  /** Switch the project page to its Previews tab, which owns the detail. */
  onOpenPreviews?: () => void;
}) {
  const tierModel = useTierModel();
  const engines = useEngines();
  const events = useRunStream(task.runId);
  const [diff, setDiff] = useState<string | null>(null);
  // The bake-off panel: same brief, several attempts, compare and keep one.
  const [bakeoff, setBakeoff] = useState(false);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [teams, setTeams] = useState<Team[]>([]);
  const [reassignError, setReassignError] = useState<string | null>(null);
  const [merging, setMerging] = useState(false);
  const [serverPending, setServerPending] = useState<PendingPermission[]>([]);
  const [answered, setAnswered] = useState<Set<string>>(new Set());
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [articleIds, setArticleIds] = useState<string[]>([]);
  // A live run opens on its transcript, not on an empty comment thread —
  // landing on "No comments yet" while an agent is mid-Bash is how the card
  // ends up looking like nothing is happening at all.
  const [panel, setPanel] = useState<"comments" | "activity">(
    isActive(task.runStatus) ? "activity" : "comments",
  );
  const att = useAttachments(task.projectId);
  const [attachBusy, setAttachBusy] = useState(false);
  const [busy, setBusy] = useState<"retry" | "delete" | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [confirm, setConfirm] = useState<{
    title: string;
    body: string;
    cta: string;
    go: () => void;
  } | null>(null);
  const shownTier = displayTier(task);
  const accent = tierColor[shownTier];
  // Anything that still owes an outcome, including a team run parked for
  // your approval — those must not look finished.
  const running = isActive(task.runStatus);

  const doRetry = async () => {
    setConfirm(null);
    setBusy("retry");
    try {
      await api.retryTask(task.id, true);
      onChanged();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(null);
    }
  };

  const retry = () => {
    // A card in review holds an unmerged diff, and a fresh retry throws it
    // away — that is worth one click of confirmation.
    if (task.boardColumn === "review") {
      setConfirm({
        title: "Retry discards the current diff",
        body: "This card has unmerged work. Retrying starts again from a clean checkout, so that diff is lost.",
        cta: "Retry anyway",
        go: doRetry,
      });
    } else {
      doRetry();
    }
  };

  const remove = () => {
    setConfirm({
      title: "Delete this card?",
      body: "Its comments, run history, attachments, and worktree branch go with it. Agents keep what they remember about the work.",
      cta: "Delete",
      go: async () => {
        setConfirm(null);
        setBusy("delete");
        try {
          await api.deleteTask(task.id);
          onChanged();
          onClose();
        } catch (e) {
          setError(String(e));
          setBusy(null);
        }
      },
    });
  };

  useEffect(() => {
    api
      .taskArticles(task.id)
      .then((r) => setArticleIds(r.articles.map((a) => a.id)))
      .catch(() => {});
  }, [task.id]);

  useEffect(() => {
    api
      .agents(workspaceId)
      .then((r) => setAgents(r.agents))
      .catch(() => {});
    api
      .teams(workspaceId)
      .then((r) => setTeams(r.teams))
      .catch(() => {});
  }, [workspaceId]);

  // Follow a run that starts while the drawer is already open. Keyed on the
  // run id so it fires once per run and never fights a manual tab click.
  const followedRun = useRef<string | null>(null);
  useEffect(() => {
    if (task.runId && isActive(task.runStatus) && followedRun.current !== task.runId) {
      followedRun.current = task.runId;
      setPanel("activity");
    }
  }, [task.runId, task.runStatus]);

  const reassign = async (next: { kind: "agent" | "team"; id: string } | null) => {
    setReassignError(null);
    try {
      await api.reassignTask(task.id, next);
      onChanged();
    } catch (e) {
      setReassignError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  useEffect(() => {
    setAttachments([]);
    api
      .taskAttachments(task.id)
      .then((r) => setAttachments(r.attachments))
      .catch(() => {});
  }, [task.id]);

  // Bind freshly-uploaded files to this card; its next run will see them.
  const commitAttachments = async () => {
    if (!att.ids.length || attachBusy) return;
    setAttachBusy(true);
    try {
      await api.attachToTask(task.id, att.ids);
      att.clear();
      const r = await api.taskAttachments(task.id);
      setAttachments(r.attachments);
    } catch {
      /* chips keep their state; user can retry */
    } finally {
      setAttachBusy(false);
    }
  };

  // Permission requests are held in memory by the broker while the engine
  // blocks on them, so a refresh has to re-fetch whatever is still open.
  const runId = task.runId;
  const refreshPending = useCallback(async () => {
    if (!runId) return setServerPending([]);
    try {
      setServerPending((await api.pendingPermissions(runId)).pending);
    } catch {
      /* transient; next tick retries */
    }
  }, [runId]);

  useEffect(() => {
    setAnswered(new Set());
    refreshPending();
    const interval = setInterval(refreshPending, 3000);
    return () => clearInterval(interval);
  }, [refreshPending]);

  // Open prompts = server-held ∪ live-streamed, minus resolved/answered.
  const openPermissions = useMemo(() => {
    const resolved = new Set(
      events
        .filter((e) => e.type === "permission_resolved")
        .map((e) => String(e.request_id)),
    );
    const merged = new Map<string, PendingPermission>();
    for (const p of serverPending) merged.set(p.requestId, p);
    for (const e of events) {
      if (e.type !== "permission_requested") continue;
      const requestId = String(e.request_id);
      merged.set(requestId, {
        requestId,
        toolName: String(e.tool_name),
        input: e.input,
      });
    }
    return [...merged.values()].filter(
      (p) => !resolved.has(p.requestId) && !answered.has(p.requestId),
    );
  }, [events, serverPending, answered]);

  const answer = async (requestId: string, allowed: boolean) => {
    setAnswered((prev) => new Set(prev).add(requestId));
    try {
      await api.resolvePermission(requestId, allowed);
    } finally {
      refreshPending();
    }
  };

  const loadDiff = async () => setDiff((await api.diff(task.id)).diff);
  const merge = async () => {
    if (merging) return;
    setMerging(true);
    setError(null);
    try {
      await api.merge(task.id);
      onChanged();
      onClose();
    } catch (e) {
      // Inline, like every other failure in this drawer. A native alert()
      // loses the drawer's context and can't be copied out of easily.
      setError(`Merge failed. ${String(e).replace(/^Error:\s*/, "")}`);
    } finally {
      setMerging(false);
    }
  };

  return (
    <motion.aside
      initial={{ x: 560 }}
      animate={{ x: 0 }}
      exit={{ x: 560 }}
      transition={{ type: "spring", stiffness: 320, damping: 34 }}
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-full max-w-[560px] flex-col border-l border-line bg-panel"
    >
      <div className="flex items-start gap-3 border-b border-line p-5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-base font-semibold">{task.title}</div>
          <div className="mt-1 flex items-center gap-2 text-xs text-ink-dim">
            <span
              className="rounded-full px-2 py-0.5"
              style={{ background: `${accent}22`, color: accent }}
            >
              {task.tierIsAuto && "auto · "}
              {tierModel(shownTier)}
            </span>
            {task.runStatus && <span>{statusLabel(task.runStatus)}</span>}
            {task.costUsd != null && <span>${task.costUsd.toFixed(3)}</span>}
          </div>
          {/* Why aichip picked this tier. Shown whenever aichip did the
              picking, because a choice made on someone's behalf that they
              cannot see is the silent downgrade this project refuses
              elsewhere — the reason travels with the run that used it. */}
          {task.tierIsAuto && task.tierReason && (
            <div className="mt-1 text-[11px] text-ink-dim/80">
              Auto → {task.tierResolved}: {task.tierReason}
            </div>
          )}
          {/* What it is doing, right in the header — visible without opening
              a tab or scrolling a transcript. */}
          <ActivityLine events={events} live={running} className="mt-1" />
        </div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">
          ✕
        </button>
      </div>

      {/* Everything between the pinned header and the pinned tabs scrolls on
          its own. Without this the card's controls, attachments and any
          queued permission prompts simply ran off the bottom of the drawer
          with no way to reach them — several prompts stacked up is exactly
          when you most need to get at them. Capped so it can never crowd out
          the transcript below. */}
      <div className="max-h-[55vh] overflow-y-auto">
      <EpicPanel task={task} boardTasks={boardTasks} onOpenTask={onOpenTask} />
      {task.runId && <PlanReviewPanel runId={task.runId} onChanged={onChanged} />}

      <div className="border-b border-line px-5 py-3">
        <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          Assigned to
        </div>
        <AssigneePicker
          value={
            task.teamId
              ? { kind: "team", id: task.teamId }
              : task.agentId
                ? { kind: "agent", id: task.agentId }
                : null
          }
          agents={agents}
          teams={teams}
          disabled={running}
          disabledReason="Cancel the run to hand this card to someone else."
          onChange={reassign}
        />
        {reassignError && (
          <div className="mt-1.5 rounded-lg bg-red-50 px-2.5 py-1.5 text-[11px] text-danger">
            {reassignError}
          </div>
        )}
        <div className="mt-3">
          <ArticlePicker
            workspaceId={workspaceId}
            selected={articleIds}
            onChange={async (ids) => {
              setArticleIds(ids);
              await api.setTaskArticles(task.id, ids);
            }}
          />
        </div>

        <label className="mt-3 flex cursor-pointer items-start gap-2 text-xs">
          <input
            type="checkbox"
            checked={task.planFirst}
            disabled={running}
            onChange={async (e) => {
              await api.moveTask(task.id, { plan_first: e.target.checked });
              onChanged();
            }}
            className="mt-0.5 accent-[var(--color-accent)]"
          />
          <span className="min-w-0">
            <span className="block font-medium">Plan first</span>
            <span className="block text-[11px] text-ink-dim">
              Write a plan and stop, so you can confirm or rewrite it before
              anything changes.
            </span>
          </span>
        </label>

        {!!engines && engines.length > 1 && (
          <div className="mt-3 flex items-center gap-2">
            <span className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              Run on
            </span>
            <EnginePicker
              value={task.engine}
              onChange={async (id) => {
                if (!id || running) return;
                await api.moveTask(task.id, { engine: id });
                onChanged();
              }}
            />
          </div>
        )}

        <div className="mt-3 flex flex-wrap items-center gap-2">
          <span className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Model
          </span>
          <CardTierPicker
            value={task.modelTier}
            engine={task.engine}
            disabled={running}
            onChange={async (t) => {
              await api.moveTask(task.id, { model_tier: t });
              onChanged();
            }}
          />
          <span className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Thinking
          </span>
          <EffortPicker
            value={task.effort}
            disabled={running}
            // Only worth naming when it comes from somewhere other than this
            // card — otherwise "Default (medium)" beside a card that says
            // medium reads as if it were set twice.
            inherited={task.effortSource === "card" ? null : task.effectiveEffort}
            onChange={async (e) => {
              await api.moveTask(task.id, { effort: e });
              onChanged();
            }}
          />
          {/* Where "Default" actually came from. Silent when the card sets it
              itself, since the picker already says so. */}
          {task.effortSource === "agent" && (
            <span className="text-[11px] text-ink-dim">
              {task.agentName
                ? `set by the ${task.agentName} agent, which outranks this card`
                : "set by its agent, which outranks this card"}
            </span>
          )}
          {task.effortSource === "tier" && (
            <span className="text-[11px] text-ink-dim">
              from the {task.modelTier} tier on {task.engine}
            </span>
          )}
        </div>

        <PreviewPanel
          taskId={task.id}
          projectId={task.projectId}
          onOpenPreviews={onOpenPreviews}
        />

        <Permissions task={task} />
      </div>

      <div className="flex gap-2 border-b border-line px-5 py-3">
        {task.orgRunId && onOpenTeamRoom && (
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => onOpenTeamRoom(task.orgRunId!)}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white"
          >
            🏛 Open team room
          </motion.button>
        )}
        {/* A bake-off answers "which agent should do this?" with evidence
            rather than a hunch, so it belongs before the work is accepted —
            not on a card that already has a diff you like. */}
        {!task.teamId && task.boardColumn !== "done" && (
          <button
            onClick={() => setBakeoff(true)}
            className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim"
          >
            ⚖ Bake-off
          </button>
        )}
        {task.runId && isWorking(task.runStatus) && (
          <button
            onClick={() => api.cancelRun(task.runId!)}
            className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-red-400 hover:text-red-400"
          >
            Cancel run
          </button>
        )}
        {task.boardColumn === "review" && (
          <>
            <button
              onClick={loadDiff}
              className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim"
            >
              View diff
            </button>
            {/* Once it is merged on GitHub, squash-merging is a trap rather
                than a shortcut: your base branch has not pulled yet, so the
                squash would write the same change again under this card's
                message. What is needed is a pull, and saying so is more use
                than a button that quietly duplicates a commit. */}
            {task.prState === "merged" ? (
              <span className="text-xs text-ink-dim">
                Merged on GitHub — <code className="font-mono">git pull</code> to update
                your checkout.
              </span>
            ) : (
              <motion.button
                whileTap={{ scale: 0.96 }}
                onClick={merge}
                disabled={merging}
                className="rounded-lg bg-tier-easy px-3 py-1.5 text-xs font-medium text-surface"
              >
                {merging ? "Merging…" : "Squash-merge"}
              </motion.button>
            )}
          </>
        )}
        {!running && (
          <button
            onClick={retry}
            disabled={busy !== null}
            className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim disabled:opacity-50"
            title="Run this card again from a clean checkout"
          >
            {busy === "retry" ? "Restarting…" : "↻ Retry"}
          </button>
        )}
        <button
          onClick={remove}
          disabled={busy !== null}
          className="ml-auto rounded-lg border border-line px-3 py-1.5 text-xs text-ink-dim hover:border-danger hover:text-danger disabled:opacity-50"
        >
          {busy === "delete" ? "Deleting…" : "Delete"}
        </button>
      </div>

      {/* Below the row rather than in it: the status line wants the full
          width, and a card keeps its pull request after it leaves review. */}
      {(task.boardColumn === "review" || task.boardColumn === "done") && (
        <div className="border-b border-line px-5 py-2">
          <PullRequestPanel taskId={task.id} />
        </div>
      )}

      {error && (
        <div className="border-b border-line bg-red-50 px-5 py-2 text-xs text-danger">
          {error}
        </div>
      )}

      {confirm && (
        <div className="border-b border-line bg-amber-50 px-5 py-3 text-xs text-amber-800">
          <div className="font-medium">{confirm.title}</div>
          <div className="mt-0.5">{confirm.body}</div>
          <div className="mt-2 flex gap-2">
            <button
              onClick={confirm.go}
              className="rounded-lg bg-danger px-3 py-1 font-medium text-white"
            >
              {confirm.cta}
            </button>
            <button onClick={() => setConfirm(null)} className="px-2 py-1 hover:underline">
              Cancel
            </button>
          </div>
        </div>
      )}

      <div className="border-b border-line px-5 py-3">
        <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
          Attachments
        </div>
        <AttachmentList attachments={attachments} />
        <div className="flex items-center gap-2">
          <AttachmentBar
            items={att.items}
            onAdd={att.add}
            onRemove={att.remove}
            full={att.full}
          />
          {att.ids.length > 0 && (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={commitAttachments}
              disabled={att.busy || attachBusy}
              className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white disabled:opacity-50"
            >
              {attachBusy ? "Attaching…" : `Attach ${att.ids.length}`}
            </motion.button>
          )}
        </div>
      </div>

      <AnimatePresence>
        {openPermissions.length > 0 && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden border-b border-line bg-amber-50"
          >
            <div className="flex flex-col gap-2 p-4">
              {openPermissions.map((p) => (
                <PermissionRow
                  key={p.requestId}
                  toolName={p.toolName}
                  input={p.input}
                  onAnswer={(allowed) => answer(p.requestId, allowed)}
                />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>
      </div>

      <div className="flex gap-1 border-b border-line px-5 py-2">
        {(["comments", "activity"] as const).map((p) => (
          <button
            key={p}
            onClick={() => setPanel(p)}
            className={`rounded-md px-3 py-1 text-xs capitalize transition-colors ${
              panel === p ? "bg-panel-2 font-medium text-ink" : "text-ink-dim"
            }`}
          >
            {p === "activity" ? "Activity" : "Comments"}
            {p === "activity" && running && (
              <motion.span
                className="ml-1.5 inline-block h-1.5 w-1.5 rounded-full bg-tier-medium align-middle"
                animate={{ opacity: [1, 0.25, 1] }}
                transition={{ duration: 1.6, repeat: Infinity }}
              />
            )}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {bakeoff ? (
          <BakeoffView
            taskId={task.id}
            agents={agents}
            currentTier={shownTier}
            onKept={onChanged}
            onClose={() => setBakeoff(false)}
          />
        ) : diff !== null ? (
          <DiffView
            diff={diff}
            taskId={task.id}
            onBack={() => setDiff(null)}
            onFixStarted={onChanged}
          />
        ) : panel === "comments" ? (
          <TaskComments taskId={task.id} />
        ) : (
          <RunStream events={events} empty="Nothing yet." />
        )}
      </div>
    </motion.aside>
  );
}

/**
 * What this card will stop to ask about, and who decided that.
 *
 * It reads as trivia until it isn't. The permission mode is resolved from three
 * places — the bound agent's preset, then the card's, then the machine default —
 * and the first one wins. So a project switched to "works without asking" goes on
 * prompting for every command if its agent carries `reviewed`, and nothing
 * anywhere said which of the three was in charge. The answer to "why is it still
 * asking me" was previously a database query.
 */
function Permissions({ task }: { task: Task }) {
  const says = {
    reviewed: "Asks before editing files or running commands",
    auto_edit: "Edits files freely · asks before running commands",
    full_auto: "Works without asking",
  }[task.effectiveMode];

  const from = {
    agent: task.agentName ? `set by the ${task.agentName} agent` : "set by its agent",
    card: "set on this card",
    default: "your default for new work",
  }[task.permissionSource];

  const asks = task.effectiveMode !== "full_auto";

  return (
    <div className="mt-3 flex items-baseline gap-2 text-[11px]">
      <span className="font-semibold uppercase tracking-wide text-ink-dim">
        Permission
      </span>
      <span className={asks ? "text-ink" : "text-tier-easy"}>{says}</span>
      {/* Naming the source is the whole point — it turns "why is this asking me"
          into a place to go and change it. */}
      <span className="text-ink-dim">· {from}</span>
    </div>
  );
}

/**
 * Where this card sits in an epic — either as the epic, or as one of its parts.
 *
 * Both directions are shown from the same panel because they are the same
 * question asked from two ends. Reading a sub-ticket, "what is this part of" is
 * the missing context; reading an epic, "what is left" is.
 */
function EpicPanel({
  task,
  boardTasks,
  onOpenTask,
}: {
  task: Task;
  boardTasks: Task[];
  onOpenTask?: (t: Task) => void;
}) {
  const parent = task.parentId
    ? boardTasks.find((t) => t.id === task.parentId)
    : undefined;
  const children = boardTasks.filter((t) => t.parentId === task.id);
  if (!parent && children.length === 0) return null;

  return (
    <div className="border-b border-line px-5 py-3">
      {parent && (
        <button
          onClick={() => onOpenTask?.(parent)}
          disabled={!onOpenTask}
          className="text-[11px] text-ink-dim hover:text-accent disabled:hover:text-ink-dim"
        >
          ↳ part of <span className="font-medium">{parent.title}</span>
        </button>
      )}
      {children.length > 0 && (
        <>
          <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Sub-tasks · {children.filter((c) => resolved(c)).length} of {children.length} done
          </div>
          <div className="space-y-1">
            {children.map((child) => (
              <button
                key={child.id}
                onClick={() => onOpenTask?.(child)}
                disabled={!onOpenTask}
                className="flex w-full items-center gap-2 rounded-lg border border-line bg-panel-2 px-2.5 py-1.5 text-left text-xs hover:border-accent disabled:hover:border-line"
              >
                <span className="min-w-0 flex-1 truncate">{child.title}</span>
                {child.agentName && (
                  <span className="shrink-0 text-[10px] text-ink-dim">{child.agentName}</span>
                )}
                <span
                  className={`shrink-0 text-[10px] ${
                    child.stepStatus === "failed" ? "text-danger" : "text-ink-dim"
                  }`}
                >
                  {child.stepStatus === "failed" ? "failed" : child.boardColumn}
                </span>
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
}

/** Reached an end state a person would call finished-with. */
const resolved = (t: Task) => t.boardColumn === "review" || t.boardColumn === "done";

/**
 * The diff, with a comment gutter.
 *
 * Clicking a line opens a note anchored to that file and line. "Ask to fix"
 * turns the note into a scoped run in this task's existing worktree, so the
 * correction lands on the same branch and shows up in this same diff —
 * which is the difference between reviewing work and re-describing it.
 */
function DiffView({
  diff,
  taskId,
  onBack,
  onFixStarted,
}: {
  diff: string;
  taskId: string;
  onBack: () => void;
  onFixStarted: () => void;
}) {
  const lines = useMemo(() => annotateDiff(diff), [diff]);
  const [openAt, setOpenAt] = useState<number | null>(null);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState(false);
  const [sent, setSent] = useState<string | null>(null);

  const submit = async (fix: boolean) => {
    if (openAt === null || !note.trim() || busy) return;
    const line = lines[openAt];
    setBusy(true);
    try {
      await api.postComment(taskId, note.trim(), undefined, undefined, {
        file_path: line.file ?? undefined,
        line: line.newLine ?? undefined,
        hunk: hunkText(lines, line.hunk),
        fix,
      });
      setNote("");
      setOpenAt(null);
      setSent(fix ? "Fix queued — it'll appear in this diff." : "Note saved to the card.");
      if (fix) onFixStarted();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div>
      <div className="mb-3 flex items-center justify-between">
        <button onClick={onBack} className="text-xs text-ink-dim hover:text-ink">
          ← back to stream
        </button>
        <span className="text-[11px] text-ink-dim">Click a line to comment on it</span>
      </div>

      {sent && (
        <div className="mb-2 rounded-lg bg-tier-easy-soft px-3 py-2 text-xs text-tier-easy">
          {sent}
        </div>
      )}

      <div className="overflow-x-auto rounded-lg bg-panel-2 py-2 font-mono text-xs leading-relaxed">
        {lines.map((line, i) => (
          <div key={i}>
            <div
              onClick={() => isCommentable(line) && setOpenAt(openAt === i ? null : i)}
              className={`group flex gap-2 px-3 ${
                isCommentable(line) ? "cursor-pointer hover:bg-panel" : ""
              } ${
                line.kind === "add"
                  ? "text-tier-easy"
                  : line.kind === "del"
                    ? "text-red-400"
                    : line.kind === "hunk"
                      ? "text-tier-medium"
                      : "text-ink-dim"
              }`}
            >
              <span className="w-8 shrink-0 select-none text-right text-ink-dim/50">
                {line.newLine ?? ""}
              </span>
              <span className="w-3 shrink-0 select-none text-ink-dim opacity-0 group-hover:opacity-100">
                {isCommentable(line) ? "+" : ""}
              </span>
              <span className="whitespace-pre">{line.text || " "}</span>
            </div>

            {openAt === i && (
              <div className="my-1 rounded-lg border border-accent/40 bg-panel p-2.5 font-sans">
                <div className="text-[11px] text-ink-dim">
                  {line.file ?? "this change"}
                  {line.newLine ? ` · line ${line.newLine}` : ""}
                </div>
                <textarea
                  autoFocus
                  value={note}
                  onChange={(e) => setNote(e.target.value)}
                  rows={2}
                  placeholder="What's wrong with this?"
                  className="mt-1.5 w-full resize-none rounded-lg border border-line bg-panel px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                />
                <div className="mt-2 flex flex-wrap gap-2">
                  <button
                    onClick={() => submit(true)}
                    disabled={busy || !note.trim()}
                    className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
                  >
                    {busy ? "…" : "Ask to fix"}
                  </button>
                  <button
                    onClick={() => submit(false)}
                    disabled={busy || !note.trim()}
                    className="rounded-lg border border-line px-3 py-1.5 text-xs disabled:opacity-50"
                  >
                    Just comment
                  </button>
                  <button
                    onClick={() => {
                      setOpenAt(null);
                      setNote("");
                    }}
                    className="px-2 text-xs text-ink-dim hover:text-ink"
                  >
                    Cancel
                  </button>
                </div>
              </div>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
