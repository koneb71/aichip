import { useState } from "react";
import { motion } from "framer-motion";
import { displayTier, Task, tierColor } from "../lib/api";
import { useTierModel } from "../lib/models";
import { ActivityLine } from "./RunStream";
import { useRunStream } from "../lib/ws";
import { prOnCard, prSummary, prTone } from "../lib/pullRequest";
import { checksChip } from "../lib/checks";
import { springy } from "../lib/motion";
import { isWorking, needsYou, statusLabel, stopReason, unblocked, unresolvedBlockers } from "../lib/runStatus";
import { RunError } from "./ui/RunError";
import { Badge, StatusDot } from "./ui/Badge";
import { Avatar } from "./ui/Avatar";
import { Progress } from "./ui/Layout";
import { cn } from "./ui/cn";
import { Building2, CornerDownRight, GitPullRequest, Hand, Lock, LockOpen, TriangleAlert, Users } from "lucide-react";

const COLUMNS: {
  key: Task["boardColumn"];
  label: string;
  /** The lane's identity dot, and the count chip when the lane has cards. */
  dot: string;
  empty: string;
}[] = [
  { key: "backlog", label: "Backlog", dot: "border-[1.5px] border-fg-subtle", empty: "Create a card to get started" },
  { key: "running", label: "In progress", dot: "bg-accent", empty: "Drag a card here to start it" },
  { key: "review", label: "Review", dot: "bg-warning", empty: "Nothing waiting for review" },
  { key: "done", label: "Done", dot: "bg-success", empty: "Nothing done yet" },
];

/** Position for a card dropped before `before` (or at the end when null). */
function dropPosition(colTasks: Task[], before: Task | null): number {
  if (!before) {
    const last = colTasks[colTasks.length - 1];
    return (last?.position ?? 0) + 10;
  }
  const i = colTasks.findIndex((t) => t.id === before.id);
  const prev = colTasks[i - 1];
  // Midpoint between neighbours; before the first card = target − 10.
  return prev ? (prev.position + before.position) / 2 : before.position - 10;
}

export function Board({
  tasks,
  onSelect,
  onMove,
}: {
  tasks: Task[];
  onSelect: (t: Task) => void;
  /** Drag-and-drop: persist column + position. Rejections surface upstream. */
  onMove: (taskId: string, column: Task["boardColumn"], position: number) => void;
}) {
  const [dragId, setDragId] = useState<string | null>(null);
  const [overCol, setOverCol] = useState<string | null>(null);

  const drop = (col: Task["boardColumn"], before: Task | null) => {
    if (!dragId) return;
    const colTasks = tasks.filter((t) => t.boardColumn === col && t.id !== dragId);
    onMove(dragId, col, dropPosition(colTasks, before));
    setDragId(null);
    setOverCol(null);
  };

  // Columns share the width when there is enough of it and scroll sideways
  // when there isn't — four columns squeezed onto a phone would fit nothing
  // but the card titles.
  return (
    <div className="grid h-full grid-cols-[repeat(4,minmax(248px,1fr))] gap-3 overflow-x-auto bg-bg p-3 sm:p-4">
      {COLUMNS.map((col) => {
        const colTasks = tasks.filter((t) => t.boardColumn === col.key);
        return (
          <div
            key={col.key}
            className={`flex min-h-0 min-w-0 flex-col rounded-lg p-1.5 transition-colors duration-200 ${
              dragId && overCol === col.key
                ? "bg-accent-subtle ring-1 ring-[color-mix(in_oklab,var(--color-accent)_45%,transparent)]"
                : "bg-[color-mix(in_oklab,var(--color-panel-2)_55%,transparent)]"
            }`}
            onDragOver={(e) => {
              e.preventDefault();
              setOverCol(col.key);
            }}
            onDragLeave={(e) => {
              if (!e.currentTarget.contains(e.relatedTarget as Node)) setOverCol(null);
            }}
            onDrop={(e) => {
              e.preventDefault();
              drop(col.key, null);
            }}
          >
            <div className="flex h-8 items-center gap-2 px-1.5">
              <span className={`size-2.5 shrink-0 rounded-full ${col.dot}`} aria-hidden />
              <span className="text-[13px] font-medium">{col.label}</span>
              <span className="tabular text-xs text-fg-subtle">{colTasks.length}</span>
              {col.key === "running" && dragId && (
                <span className="text-[10px] font-medium text-accent-fg">drop to start</span>
              )}
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-1.5 overflow-y-auto px-px pb-2 pt-0.5">
              {colTasks.map((task) => (
                <div
                  key={task.id}
                  draggable
                  onDragStart={(e) => {
                    setDragId(task.id);
                    e.dataTransfer.effectAllowed = "move";
                  }}
                  onDragEnd={() => {
                    setDragId(null);
                    setOverCol(null);
                  }}
                  onDrop={(e) => {
                    // Dropping on a card inserts before it.
                    e.preventDefault();
                    e.stopPropagation();
                    drop(col.key, task);
                  }}
                  className={dragId === task.id ? "opacity-40" : ""}
                >
                  <TaskCard task={task} onSelect={onSelect} />
                </div>
              ))}
              {colTasks.length === 0 && (
                <div className="mt-1 rounded-md border border-dashed border-border px-3 py-6 text-center text-xs text-fg-subtle">
                  {col.empty}
                </div>
              )}
            </div>
          </div>
        );
      })}
    </div>
  );
}

function TaskCard({
  task,
  onSelect,
}: {
  task: Task;
  onSelect: (t: Task) => void;
}) {
  const tierModel = useTierModel();
  const shown = displayTier(task);
  const accent = task.agentColor ?? tierColor[shown];
  const teamRun = !!task.teamName;
  const running = isWorking(task.runStatus);
  // `needsYou` covers both parked states. Hand-rolling this covered only the
  // tool prompt, so a plan waiting for approval — the one that genuinely *is*
  // an approval — drew no badge and no ring at all.
  const waiting = needsYou(task.runStatus);
  const stopped = stopReason(task.runStatus, task.runError);

  const blockers = unresolvedBlockers(task);
  const chip = checksChip(task.localChecks);
  const pr = prOnCard(task);
  const cost = task.totalCostUsd ?? task.costUsd;

  return (
    <motion.button
      layout
      layoutId={task.id}
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.98 }}
      whileTap={{ scale: 0.995 }}
      onClick={() => onSelect(task)}
      transition={springy}
      // `w-full min-w-0` is load-bearing: a button sizes to its content, and a
      // running card's activity line carries an unbreakable worktree path —
      // without a constrained width the card grows past its column.
      className={cn(
        "ring-focus group relative block w-full min-w-0 overflow-hidden rounded-lg border bg-panel px-2.5 py-2 text-left shadow-[var(--shadow-xs)] transition-[box-shadow,border-color] duration-[var(--dur-fast)] hover:border-border-strong hover:shadow-[var(--shadow-sm)]",
        running
          ? "border-[color-mix(in_oklab,var(--color-accent)_45%,var(--color-border))]"
          : waiting
            ? "border-[color-mix(in_oklab,var(--color-warning)_50%,var(--color-border))]"
            : "border-border",
      )}
    >
      {running && (
        <motion.span
          aria-hidden
          className="absolute inset-x-0 top-0 h-px origin-left"
          style={{ background: `linear-gradient(90deg, transparent, ${accent}, transparent)` }}
          animate={{ x: ["-100%", "100%"] }}
          transition={{ duration: 1.8, repeat: Infinity, ease: "linear" }}
        />
      )}

      {/* Which epic this belongs to, above its own title — a sub-ticket read on
          its own says what to do but not what it is part of. */}
      {task.parentTitle && (
        <div className="mb-0.5 flex items-center gap-1 truncate pr-5 text-[11px] text-fg-subtle" title={task.parentTitle}>
          <CornerDownRight className="size-3 shrink-0" />
          <span className="truncate">{task.parentTitle}</span>
        </div>
      )}
      <div className="flex items-start gap-2">
        <div className="line-clamp-2 min-w-0 flex-1 text-[13px] font-medium leading-snug text-fg">{task.title}</div>
        {running && <StatusDot tone="accent" pulse className="mt-1" label="Running" />}
      </div>
      {waiting && (
        <div className="mt-1.5" title={stopped?.tone === "note" ? stopped.text : undefined}>
          <Badge tone="warning" icon={<Hand className="size-3" />}>
            {statusLabel(task.runStatus)}
          </Badge>
        </div>
      )}
      {running && <CardActivity runId={task.runId} />}

      {/* An epic's own progress, derived from the children's columns. */}
      {task.childCount > 0 && (
        <div className="mt-2 flex items-center gap-2">
          <Progress value={task.childResolved} max={task.childCount} tone={task.childResolved === task.childCount ? "success" : "accent"} label="Sub-cards done" />
          <span className="tabular shrink-0 text-[11px] text-fg-muted">
            {task.childResolved}/{task.childCount}
          </span>
        </div>
      )}

      {stopped && stopped.tone !== "note" && <RunError reason={stopped.text} tone={stopped.tone} compact className="mt-2" />}

      <div className="mt-2 flex flex-wrap items-center gap-1">
        <StepOutcome status={task.stepStatus} />
        {task.blockedNote && blockers.length === 0 ? (
          <Badge tone="warning" icon={<TriangleAlert className="size-3" />} title={task.blockedNote}>
            stuck
          </Badge>
        ) : blockers.length > 0 ? (
          <Badge
            tone="warning"
            icon={<Lock className="size-3" />}
            title={`Waiting for: ${blockers.map((b) => b.title).join(", ")}${task.startWhenUnblocked ? " — starts by itself when they land" : ""}`}
          >
            blocked{blockers.length > 1 ? ` · ${blockers.length}` : ""}
            {task.startWhenUnblocked && " · auto"}
          </Badge>
        ) : unblocked(task) ? (
          <Badge tone="success" icon={<LockOpen className="size-3" />} title="Everything it was waiting for has landed — it can start">
            unblocked
          </Badge>
        ) : null}
        {!teamRun && (
          <Badge tone={shown} title={task.tierIsAuto ? "Tier picked automatically for each run" : undefined}>
            {task.tierIsAuto && "auto · "}
            {tierModel(shown)}
          </Badge>
        )}
        {chip && (
          <Badge
            tone={{ good: "success", bad: "danger", busy: "neutral", warn: "warning" }[chip.tone] as "success"}
            title={chip.title}
            className="tabular"
          >
            {chip.label}
          </Badge>
        )}
        {pr && (
          <Badge tone="neutral" icon={<GitPullRequest className="size-3" />} title={`Pull request #${pr.number} — ${prSummary(pr)}`}>
            <span className={prTone(pr).text}>#{pr.number}</span>
          </Badge>
        )}
      </div>

      {(task.agentName || task.teamName || cost != null) && (
        <div className="mt-2 flex items-center gap-1.5 border-t border-border pt-1.5 text-[11px] text-fg-muted">
          {task.agentName && (
            <span
              className={cn("flex min-w-0 items-center gap-1.5", task.agentStatus === "paused" && "opacity-60")}
              title={task.agentStatus === "paused" ? `${task.agentName} is paused — this card will not start` : task.agentName}
            >
              <Avatar name={task.agentName} color={task.agentColor} size={16} />
              <span className="truncate">{task.agentName}</span>
              {task.agentStatus === "paused" && <span>· paused</span>}
            </span>
          )}
          {task.teamName && (
            <span className="flex min-w-0 items-center gap-1" title={`Assigned to the ${task.teamName} ${task.teamPattern}`}>
              {task.teamPattern === "org" ? <Building2 className="size-3.5" /> : <Users className="size-3.5" />}
              <span className="truncate">{task.teamName}</span>
            </span>
          )}
          {/* Every run's dollars, not only the newest — a retry costs money too. */}
          {cost != null && (
            <span className="tabular ml-auto font-mono" title={(task.runCount ?? 0) > 1 ? `over ${task.runCount} runs` : undefined}>
              ${cost.toFixed(3)}
            </span>
          )}
        </div>
      )}
    </motion.button>
  );
}

/**
 * What became of the assignment behind this card.
 *
 * Only for the outcomes a column cannot express. "Done" and "in progress" are
 * already said by which column the card is in; repeating them here would be
 * noise. A failure parked in Review looks identical to finished work waiting to
 * be read — this is the difference.
 */
function StepOutcome({ status }: { status: string | null }) {
  if (status !== "failed" && status !== "canceled" && status !== "skipped") return null;
  const failed = status !== "skipped";
  return (
    <Badge
      tone={failed ? "danger" : "neutral"}
      title={
        failed
          ? "This assignment did not finish. Open it to see how far it got."
          : "The manager dropped this assignment — nothing was done."
      }
    >
      {status === "failed" ? "failed" : status === "canceled" ? "canceled" : "dropped"}
    </Badge>
  );
}

/** A live card's current action.
 *
 * Split into its own component so the websocket is only opened while a card
 * is actually running — a board of finished cards each holding a socket open
 * would be a lot of connections to say nothing.
 */
function CardActivity({ runId }: { runId: string | null }) {
  const events = useRunStream(runId);
  return <ActivityLine events={events} live className="mt-1.5" />;
}
