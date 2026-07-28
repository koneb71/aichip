import { useState } from "react";
import { motion } from "framer-motion";
import { Task, tierColor, tierModel, tierSoft } from "../lib/api";

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

  return (
    <div className="grid h-full grid-cols-4 gap-4 overflow-x-auto bg-surface p-5">
      {COLUMNS.map((col) => {
        const colTasks = tasks.filter((t) => t.boardColumn === col.key);
        return (
          <div
            key={col.key}
            className={`flex min-h-0 min-w-52 flex-col rounded-xl transition-colors ${
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
      <div className="pr-5 text-sm font-medium leading-snug">{task.title}</div>
      <div className="mt-2 flex flex-wrap items-center gap-1.5 text-xs text-ink-dim">
        {!teamRun && (
          <span
            className="rounded-full px-2 py-0.5"
            style={{ background: tierSoft[task.modelTier], color: tierColor[task.modelTier] }}
          >
            {tierModel[task.modelTier]}
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
