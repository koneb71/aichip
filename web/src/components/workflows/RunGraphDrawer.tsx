import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, RunStep, WorkflowRun } from "../../lib/api";
import { Markdown } from "../Markdown";
import { StatusDot } from "./WorkflowsPanel";

/** Live view of a workflow run: every step, its state, and its output.
 *  Fan-out attempts (`step#1`, `step#2`) group under their step. */
export function RunGraphDrawer({
  run,
  onClose,
}: {
  run: WorkflowRun;
  onClose: () => void;
}) {
  const [steps, setSteps] = useState<RunStep[]>([]);
  const [expanded, setExpanded] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    try {
      setSteps((await api.runSteps(run.id)).steps);
    } catch {
      /* transient */
    }
  }, [run.id]);

  useEffect(() => {
    refresh();
    const interval = setInterval(refresh, 2000);
    return () => clearInterval(interval);
  }, [refresh]);

  // Group attempts of a fanned-out step together.
  const groups = steps.reduce<Record<string, RunStep[]>>((acc, s) => {
    const base = s.stepKey.split("#")[0];
    (acc[base] ??= []).push(s);
    return acc;
  }, {});

  return (
    <motion.aside
      initial={{ x: 560 }}
      animate={{ x: 0 }}
      exit={{ x: 560 }}
      transition={{ type: "spring", stiffness: 320, damping: 34 }}
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-[560px] flex-col border-l border-line bg-panel"
    >
      <div className="flex items-start gap-3 border-b border-line p-5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-base font-semibold">{run.workflowName}</div>
          <div className="mt-1 flex items-center gap-2 text-xs text-ink-dim">
            <StatusDot status={run.status} />
            <span>{run.status.replace("_", " ")}</span>
            <span>· {run.trigger}</span>
            {run.costUsd != null && <span>· ${run.costUsd.toFixed(3)}</span>}
          </div>
        </div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">
          ✕
        </button>
      </div>

      {run.error && (
        <div className="mx-5 mt-4 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
          {run.error}
        </div>
      )}

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {Object.entries(groups).map(([base, attempts], groupIndex) => (
          <div key={base}>
            {groupIndex > 0 && (
              <div className="ml-[9px] h-4 w-px bg-line" aria-hidden />
            )}
            <div className="rounded-xl border border-line bg-panel-2/50 p-3">
              <div className="flex items-center gap-2">
                <StatusDot status={groupStatus(attempts)} />
                <span className="font-mono text-sm font-medium">{base}</span>
                {attempts.length > 1 && (
                  <span className="rounded-full bg-panel px-2 py-0.5 text-[11px] text-ink-dim">
                    {attempts.length} attempts in parallel
                  </span>
                )}
                <span className="ml-auto text-[11px] text-ink-dim">
                  {duration(attempts)}
                </span>
              </div>
              <div className="mt-2 flex flex-col gap-1.5">
                {attempts.map((s) => (
                  <button
                    key={s.id}
                    onClick={() => setExpanded(expanded === s.id ? null : s.id)}
                    className="rounded-lg bg-panel px-2.5 py-1.5 text-left"
                  >
                    <div className="flex items-center gap-2">
                      <StatusDot status={s.status} />
                      <span className="font-mono text-xs">{s.stepKey}</span>
                      <span className="ml-auto text-[11px] text-ink-dim">
                        {expanded === s.id ? "hide output" : "show output"}
                      </span>
                    </div>
                    {expanded === s.id && (
                      <motion.div
                        initial={{ opacity: 0, height: 0 }}
                        animate={{ opacity: 1, height: "auto" }}
                        className="mt-2 border-t border-line pt-2 text-sm"
                      >
                        {s.output ? (
                          <Markdown>{s.output}</Markdown>
                        ) : (
                          <span className="text-xs text-ink-dim">No output yet.</span>
                        )}
                      </motion.div>
                    )}
                  </button>
                ))}
              </div>
            </div>
          </div>
        ))}
        {steps.length === 0 && (
          <div className="text-sm text-ink-dim">
            Waiting for the first step to start…
          </div>
        )}
      </div>
    </motion.aside>
  );
}

function groupStatus(attempts: RunStep[]): string {
  if (attempts.some((a) => a.status === "failed")) return "failed";
  if (attempts.some((a) => a.status === "running")) return "running";
  if (attempts.every((a) => a.status === "completed")) return "completed";
  return attempts[0]?.status ?? "queued";
}

function duration(attempts: RunStep[]): string {
  const starts = attempts.map((a) => a.startedAt).filter(Boolean) as string[];
  const ends = attempts.map((a) => a.finishedAt).filter(Boolean) as string[];
  if (starts.length === 0) return "";
  const start = Math.min(...starts.map((s) => new Date(s).getTime()));
  const end =
    ends.length === attempts.length
      ? Math.max(...ends.map((e) => new Date(e).getTime()))
      : Date.now();
  const secs = Math.round((end - start) / 1000);
  return secs < 60 ? `${secs}s` : `${Math.floor(secs / 60)}m ${secs % 60}s`;
}
