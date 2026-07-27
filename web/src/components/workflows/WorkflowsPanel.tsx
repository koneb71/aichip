import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, WorkflowDef, WorkflowRun } from "../../lib/api";
import { RunGraphDrawer } from "./RunGraphDrawer";
import { WorkflowEditor } from "./WorkflowEditor";

export function WorkflowsPanel({ projectId }: { projectId: string }) {
  const [workflows, setWorkflows] = useState<WorkflowDef[]>([]);
  const [runs, setRuns] = useState<WorkflowRun[]>([]);
  const [editing, setEditing] = useState<WorkflowDef | "new" | null>(null);
  const [openRun, setOpenRun] = useState<WorkflowRun | null>(null);
  const [notice, setNotice] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      const [w, r] = await Promise.all([
        api.workflows(projectId),
        api.workflowRuns(projectId),
      ]);
      setWorkflows(w.workflows);
      setRuns(r.runs);
    } catch {
      /* transient */
    }
  }, [projectId]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 2500);
    return () => clearInterval(interval);
  }, [refresh]);

  const sync = async () => {
    const r = await api.syncWorkflows(projectId);
    setNotice(
      r.note ??
        `Imported ${r.imported.length} workflow${r.imported.length === 1 ? "" : "s"}` +
          (r.errors.length ? ` · ${r.errors.length} failed to parse` : ""),
    );
    refresh();
  };

  const runNow = async (id: string) => {
    const { runId } = await api.runWorkflow(id);
    await refresh();
    const started = (await api.workflowRuns(projectId)).runs.find((r) => r.id === runId);
    if (started) setOpenRun(started);
  };

  return (
    <div className="h-full overflow-y-auto bg-surface p-5">
      <div className="flex items-center gap-2">
        <h2 className="text-sm font-semibold">Workflows</h2>
        <span className="rounded-full bg-panel-2 px-2 py-0.5 text-xs text-ink-dim">
          {workflows.length}
        </span>
        <div className="ml-auto flex gap-2">
          <button
            onClick={sync}
            className="rounded-lg border border-line bg-panel px-3 py-1.5 text-xs hover:bg-panel-2"
          >
            Sync from repo
          </button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setEditing("new")}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white"
          >
            + New workflow
          </motion.button>
        </div>
      </div>

      {notice && (
        <div className="mt-3 rounded-lg border border-line bg-panel px-3 py-2 text-xs text-ink-dim">
          {notice}
        </div>
      )}

      <div className="mt-4 grid grid-cols-2 gap-3 xl:grid-cols-3">
        {workflows.map((w) => (
          <motion.div
            layout
            key={w.id}
            className="card-shadow rounded-xl border border-line bg-panel p-4"
          >
            <div className="flex items-start gap-2">
              <div className="min-w-0 flex-1">
                <div className="truncate text-sm font-semibold">{w.name}</div>
                <div className="mt-0.5 line-clamp-2 text-xs text-ink-dim">
                  {w.description || `${w.stepCount} steps`}
                </div>
              </div>
              {w.cronExpr && (
                <button
                  title={
                    w.enabled
                      ? "Scheduled — click to pause"
                      : "Paused — click to resume"
                  }
                  onClick={async () => {
                    await api.setWorkflowEnabled(w.id, !w.enabled);
                    refresh();
                  }}
                  className="shrink-0 rounded-full px-2 py-0.5 text-[11px] font-mono"
                  style={
                    w.enabled
                      ? {
                          background: "var(--color-tier-medium-soft)",
                          color: "var(--color-tier-medium)",
                        }
                      : {
                          background: "var(--color-panel-2)",
                          color: "var(--color-ink-dim)",
                        }
                  }
                >
                  {w.enabled ? "⏱" : "⏸"} {w.cronExpr}
                </button>
              )}
            </div>
            {w.error ? (
              <div className="mt-2 rounded-lg bg-red-50 px-2 py-1 text-[11px] text-danger">
                {w.error}
              </div>
            ) : (
              <div className="mt-2 text-[11px] text-ink-dim">
                {w.stepCount} steps
                {w.nextRunAt && ` · next ${relativeTime(w.nextRunAt)}`}
                {!w.nextRunAt && w.cronExpr && !w.enabled && " · paused"}
              </div>
            )}
            <div className="mt-3 flex gap-2">
              <motion.button
                whileTap={{ scale: 0.96 }}
                onClick={() => runNow(w.id)}
                disabled={!!w.error}
                className="rounded-lg bg-accent px-3 py-1 text-xs font-medium text-white disabled:opacity-40"
              >
                ▶ Run
              </motion.button>
              <button
                onClick={() => setEditing(w)}
                className="rounded-lg border border-line px-3 py-1 text-xs hover:bg-panel-2"
              >
                Edit
              </button>
            </div>
          </motion.div>
        ))}
        {workflows.length === 0 && (
          <div className="col-span-full rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
            No workflows yet. Write one here, or drop YAML in
            <code className="mx-1 rounded bg-panel-2 px-1.5 py-0.5 text-xs">
              .aichip/workflows/
            </code>
            and hit “Sync from repo”.
          </div>
        )}
      </div>

      <h2 className="mt-8 text-sm font-semibold">Recent runs</h2>
      <div className="mt-3 flex flex-col gap-1.5">
        {runs.map((r) => (
          <motion.button
            layout
            key={r.id}
            onClick={() => setOpenRun(r)}
            className="card-shadow flex items-center gap-3 rounded-xl border border-line bg-panel px-4 py-2.5 text-left"
          >
            <StatusDot status={r.status} />
            <span className="text-sm font-medium">{r.workflowName}</span>
            <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px] text-ink-dim">
              {r.trigger}
            </span>
            <span className="text-xs text-ink-dim">{r.status.replace("_", " ")}</span>
            {r.costUsd != null && (
              <span className="text-xs text-ink-dim">${r.costUsd.toFixed(3)}</span>
            )}
            <span className="ml-auto text-xs text-ink-dim">
              {new Date(r.createdAt).toLocaleTimeString()}
            </span>
          </motion.button>
        ))}
        {runs.length === 0 && (
          <div className="rounded-xl border border-dashed border-line p-6 text-center text-xs text-ink-dim">
            No workflow runs yet.
          </div>
        )}
      </div>

      <AnimatePresence>
        {editing && (
          <WorkflowEditor
            projectId={projectId}
            workflow={editing === "new" ? null : editing}
            onClose={() => setEditing(null)}
            onSaved={() => {
              setEditing(null);
              refresh();
            }}
          />
        )}
        {openRun && (
          <RunGraphDrawer
            run={openRun}
            workflow={workflows.find((w) => w.id === openRun.workflowId)}
            onClose={() => setOpenRun(null)}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

/** "in 4h", "in 2d", "now" — enough for a schedule badge. */
function relativeTime(iso: string): string {
  const seconds = Math.round((new Date(iso).getTime() - Date.now()) / 1000);
  if (seconds <= 60) return "now";
  const units: [number, string][] = [
    [60, "m"],
    [3600, "h"],
    [86400, "d"],
  ];
  const [divisor, suffix] =
    seconds < 3600 ? units[0] : seconds < 86400 ? units[1] : units[2];
  return `in ${Math.round(seconds / divisor)}${suffix}`;
}

export function StatusDot({ status }: { status: string }) {
  const color =
    status === "completed"
      ? "var(--color-tier-easy)"
      : status === "failed"
        ? "var(--color-danger)"
        : status === "canceled"
          ? "var(--color-ink-dim)"
          : "var(--color-tier-medium)";
  const live = status === "running" || status === "starting" || status === "queued";
  return live ? (
    <motion.span
      className="h-2.5 w-2.5 shrink-0 rounded-full"
      style={{ background: color }}
      animate={{ opacity: [1, 0.3, 1] }}
      transition={{ repeat: Infinity, duration: 1.6 }}
    />
  ) : (
    <span className="h-2.5 w-2.5 shrink-0 rounded-full" style={{ background: color }} />
  );
}
