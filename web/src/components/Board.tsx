import { useState } from "react";
import { motion } from "framer-motion";
import { Task, tierColor, tierSoft } from "../lib/api";
import { useTierModel } from "../lib/models";
import { ActivityLine } from "./RunStream";
import { useRunStream } from "../lib/ws";

const COLUMNS: { key: Task["boardColumn"]; label: string }[] = [
  { key: "backlog", label: "Backlog" },
  { key: "running", label: "In Progress" },
  { key: "review", label: "Review" },
  { key: "done", label: "Done" },
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
    <div className="grid h-full grid-cols-[repeat(4,minmax(240px,1fr))] gap-3 overflow-x-auto bg-surface p-3 sm:gap-4 sm:p-5">
      {COLUMNS.map((col) => {
        const colTasks = tasks.filter((t) => t.boardColumn === col.key);
        return (
          <div
            key={col.key}
            className={`flex min-h-0 min-w-0 flex-col rounded-xl transition-colors ${
              dragId && overCol === col.key ? "bg-panel-2/60 ring-1 ring-accent/40" : ""
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
            <div className="flex items-center gap-2 px-1 pb-2">
              <span className="text-sm font-semibold">{col.label}</span>
              <span className="rounded-full bg-panel-2 px-2 py-0.5 text-xs text-ink-dim">
                {colTasks.length}
              </span>
              {col.key === "running" && dragId && (
                <span className="text-[10px] text-accent">drop to start</span>
              )}
            </div>
            <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto pb-3">
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
                <div className="mt-4 rounded-xl border border-dashed border-line py-6 text-center text-xs text-ink-dim/70">
                  {col.key === "backlog" ? "Create a task to get started" : "—"}
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
  const accent = task.agentColor ?? tierColor[task.modelTier];
  const teamRun = !!task.teamName;
  const running = task.runStatus === "running" || task.runStatus === "starting";
  const waiting = task.runStatus === "waiting_permission";

  return (
    <motion.button
      layout
      layoutId={task.id}
      initial={{ opacity: 0, scale: 0.97 }}
      animate={{ opacity: 1, scale: 1 }}
      exit={{ opacity: 0, scale: 0.97 }}
      whileHover={{ y: -2 }}
      whileTap={{ scale: 0.99 }}
      onClick={() => onSelect(task)}
      className="card-shadow relative rounded-xl border border-line bg-panel p-3 text-left"
      style={
        running
          ? { boxShadow: `0 0 0 1.5px ${accent}66, 0 1px 8px ${accent}22` }
          : undefined
      }
    >
      {running && (
        <motion.span
          className="absolute right-3 top-3 h-2 w-2 rounded-full"
          style={{ background: accent }}
          animate={{ opacity: [1, 0.3, 1] }}
          transition={{ repeat: Infinity, duration: 1.6 }}
        />
      )}
      {waiting && (
        <span className="absolute right-3 top-3 text-xs text-amber-600">⏸ approval</span>
      )}
      {/* Which epic this belongs to, above its own title — a sub-ticket read on
          its own says what to do but not what it is part of. */}
      {task.parentTitle && (
        <div className="mb-0.5 truncate pr-5 text-[11px] text-ink-dim" title={task.parentTitle}>
          ↳ {task.parentTitle}
        </div>
      )}
      <div className="pr-5 text-sm font-medium leading-snug">{task.title}</div>
      {running && <CardActivity runId={task.runId} />}

      {/* An epic's own progress. Derived from the children's columns, so it
          still reads correctly long after the run that created them is gone. */}
      {task.childCount > 0 && (
        <div className="mt-2">
          <div className="flex items-center justify-between text-[11px] text-ink-dim">
            <span>
              {task.childResolved} of {task.childCount} done
            </span>
            {task.childResolved === task.childCount && <span>✓</span>}
          </div>
          <div className="mt-1 h-1 overflow-hidden rounded-full bg-panel-2">
            <motion.div
              className="h-full rounded-full"
              style={{ background: accent }}
              initial={false}
              animate={{
                width: `${(task.childResolved / task.childCount) * 100}%`,
              }}
              transition={{ type: "spring", stiffness: 200, damping: 30 }}
            />
          </div>
        </div>
      )}

      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-ink-dim">
        <StepOutcome status={task.stepStatus} />
        {!teamRun && (
          <span
            className="rounded-full px-2 py-0.5"
            style={{ background: tierSoft[task.modelTier], color: tierColor[task.modelTier] }}
          >
            {tierModel(task.modelTier)}
          </span>
        )}
        {task.agentName && (
          <span
            className="rounded-full px-2 py-0.5 text-white"
            style={{ background: task.agentColor ?? "#9ca3af" }}
          >
            {task.agentName}
          </span>
        )}
        {task.teamName && (
          <span
            className="rounded-full bg-panel-2 px-2 py-0.5"
            title={`Assigned to the ${task.teamName} ${task.teamPattern}`}
          >
            {task.teamPattern === "org" ? "🏛" : "👥"} {task.teamName}
          </span>
        )}
        {task.costUsd != null && <span>${task.costUsd.toFixed(3)}</span>}
      </div>
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
    <span
      className={`rounded-full px-2 py-0.5 font-medium ${
        failed ? "bg-red-50 text-danger" : "bg-panel-2 text-ink-dim"
      }`}
      title={
        failed
          ? "This assignment did not finish. Open it to see how far it got."
          : "The manager dropped this assignment — nothing was done."
      }
    >
      {status === "failed" ? "failed" : status === "canceled" ? "canceled" : "dropped"}
    </span>
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
