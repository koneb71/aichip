import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Agent, api, Attachment, PendingPermission, Task, tierColor, tierModel } from "../lib/api";
import { useRunStream, StreamEvent } from "../lib/ws";
import { isActive, isWorking, statusLabel } from "../lib/runStatus";
import { useAttachments } from "../lib/useAttachments";
import { AttachmentBar, AttachmentList } from "./AttachmentBar";
import { TaskComments } from "./TaskComments";
import { Markdown } from "./Markdown";
import { annotateDiff, hunkText, isCommentable } from "../lib/diff";
import { PermissionRow } from "./PermissionRow";
import { BakeoffView } from "./BakeoffView";

export function TaskDrawer({
  task,
  workspaceId,
  onClose,
  onChanged,
  onOpenTeamRoom,
}: {
  task: Task;
  /** Bounds which agents a bake-off may choose between. */
  workspaceId: string;
  onClose: () => void;
  onChanged: () => void;
  onOpenTeamRoom?: (runId: string) => void;
}) {
  const events = useRunStream(task.runId);
  const [diff, setDiff] = useState<string | null>(null);
  // The bake-off panel: same brief, several attempts, compare and keep one.
  const [bakeoff, setBakeoff] = useState(false);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [merging, setMerging] = useState(false);
  const [serverPending, setServerPending] = useState<PendingPermission[]>([]);
  const [answered, setAnswered] = useState<Set<string>>(new Set());
  const [attachments, setAttachments] = useState<Attachment[]>([]);
  const [panel, setPanel] = useState<"comments" | "activity">("comments");
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
  const accent = tierColor[task.modelTier];
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
      .agents(workspaceId)
      .then((r) => setAgents(r.agents))
      .catch(() => {});
  }, [workspaceId]);

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
    setMerging(true);
    try {
      await api.merge(task.id);
      onChanged();
      onClose();
    } catch (e) {
      alert(`Merge failed:\n${e}`);
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
              {tierModel[task.modelTier]}
            </span>
            {task.runStatus && <span>{statusLabel(task.runStatus)}</span>}
            {task.costUsd != null && <span>${task.costUsd.toFixed(3)}</span>}
          </div>
        </div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">
          ✕
        </button>
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
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={merge}
              disabled={merging}
              className="rounded-lg bg-tier-easy px-3 py-1.5 text-xs font-medium text-surface"
            >
              {merging ? "Merging…" : "Squash-merge"}
            </motion.button>
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

      <div className="flex gap-1 border-b border-line px-5 py-2">
        {(["comments", "activity"] as const).map((p) => (
          <button
            key={p}
            onClick={() => setPanel(p)}
            className={`rounded-md px-3 py-1 text-xs capitalize transition-colors ${
              panel === p ? "bg-panel-2 font-medium text-ink" : "text-ink-dim"
            }`}
          >
            {p}
          </button>
        ))}
      </div>

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {bakeoff ? (
          <BakeoffView
            taskId={task.id}
            agents={agents}
            currentTier={task.modelTier}
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
          <EventStream events={events} />
        )}
      </div>
    </motion.aside>
  );
}

function EventStream({ events }: { events: StreamEvent[] }) {
  if (events.length === 0) {
    return <div className="text-sm text-ink-dim">No events yet.</div>;
  }
  return (
    <div className="flex flex-col gap-2">
      {events.map((e, i) => (
        <EventRow key={`${e.seq}-${i}`} event={e} />
      ))}
    </div>
  );
}

function EventRow({ event }: { event: StreamEvent }) {
  const base = "rounded-lg px-3 py-2 text-sm";
  switch (event.type) {
    case "run_started":
      return (
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className={`${base} text-xs text-ink-dim`}>
          ▶ session started {String(event.model ?? "")}
        </motion.div>
      );
    case "assistant_text":
      return (
        <motion.div
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          className={`${base} bg-panel-2`}
        >
          <Markdown>{String(event.text)}</Markdown>
        </motion.div>
      );
    case "tool_call":
      return (
        <motion.div
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          className={`${base} border border-line font-mono text-xs text-ink-dim`}
        >
          ⚙ {String(event.tool_name)}{" "}
          <span className="opacity-70">
            {JSON.stringify(event.input).slice(0, 140)}
          </span>
        </motion.div>
      );
    case "tool_result":
      return (
        <div className={`${base} font-mono text-xs ${event.is_error ? "text-red-400" : "text-ink-dim/80"}`}>
          ↳ {String(event.summary).slice(0, 200)}
        </div>
      );
    // Open prompts render in the sticky banner above; the log keeps only a
    // trace so the transcript stays readable.
    case "permission_requested":
      return (
        <div className={`${base} text-xs text-ink-dim`}>
          ⏸ asked to run {String(event.tool_name)}
        </div>
      );
    case "permission_resolved":
      return (
        <div className={`${base} text-xs text-ink-dim`}>
          {event.allowed ? "✓ you allowed it" : "✗ you denied it"}
        </div>
      );
    case "run_completed":
      return (
        <motion.div
          initial={{ scale: 0.97, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          className={`${base} border border-tier-easy/40 bg-tier-easy/10 text-tier-easy`}
        >
          <div className="flex gap-1.5">
            <span>✓</span>
            <Markdown>{String(event.result_text)}</Markdown>
          </div>
        </motion.div>
      );
    case "run_failed":
      return (
        <div className={`${base} border border-red-400/40 bg-red-400/10 text-red-400`}>
          ✗ {String(event.reason)}
        </div>
      );
    case "rate_limited":
      return (
        <div className={`${base} border border-amber-400/40 bg-amber-400/10 text-amber-600`}>
          ⏳ rate-limited — re-queued automatically
        </div>
      );
    default:
      return null;
  }
}

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
      await api.postComment(taskId, note.trim(), undefined, {
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
