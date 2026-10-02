import { useEffect, useState } from "react";
import { api, TaskRun } from "../lib/api";
import { statusColor, statusLabel, stopReason } from "../lib/runStatus";
import { dollars, duration, reportExcerpt, tokensLine, triggerLabel } from "../lib/runHistory";
import { RunError } from "./ui/RunError";

/**
 * Every run of a card, newest first.
 *
 * The board shows one run per card — the newest — so a card that failed twice
 * and then succeeded looked like it had simply succeeded, and its dollars were
 * the last attempt's alone. Everything below was already being stored; this is
 * the first place it is read.
 */
export function RunHistory({
  taskId,
  latestRunId,
  latestStatus,
  viewing,
  onView,
}: {
  taskId: string;
  /** Refetch when the card's newest run changes or moves on. */
  latestRunId: string | null;
  latestStatus: string | null;
  /** The run whose transcript the Activity tab is showing, if not the latest. */
  viewing: string | null;
  onView: (runId: string) => void;
}) {
  const [runs, setRuns] = useState<TaskRun[] | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [copied, setCopied] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    api
      .taskRuns(taskId)
      .then((r) => !cancelled && (setRuns(r.runs), setError(null)))
      .catch((e) => !cancelled && setError(String(e.message ?? e)));
    return () => {
      cancelled = true;
    };
  }, [taskId, latestRunId, latestStatus]);

  if (error) return <RunError reason={error} />;
  if (!runs) return <p className="text-xs text-ink-dim">Loading…</p>;
  if (runs.length === 0) return <p className="text-xs text-ink-dim">This card has not run yet.</p>;

  const total = runs.reduce((sum, r) => sum + (r.costUsd ?? 0), 0);
  const priced = runs.some((r) => r.costUsd != null);

  const copy = (run: TaskRun) => {
    if (!run.resumeCommand) return;
    navigator.clipboard
      ?.writeText(run.resumeCommand)
      .then(() => {
        setCopied(run.runId);
        setTimeout(() => setCopied((c) => (c === run.runId ? null : c)), 1500);
      })
      .catch(() => {});
  };

  return (
    <div>
      <div className="mb-3 text-[11px] text-ink-dim">
        {runs.length} {runs.length === 1 ? "run" : "runs"}
        {priced && <> · {dollars(total)} in all</>}
      </div>
      <ol className="space-y-2">
        {runs.map((run, i) => {
          const stopped = stopReason(run.status, run.error);
          const shown = viewing ? viewing === run.runId : i === 0;
          const tokens = tokensLine(run);
          const excerpt = reportExcerpt(run.report);
          return (
            <li
              key={run.runId}
              className={`rounded-lg border p-3 ${shown ? "border-accent/50 bg-panel-2" : "border-line"}`}
            >
              <div className="flex flex-wrap items-center gap-2">
                <span
                  className="h-2 w-2 shrink-0 rounded-full"
                  style={{ background: statusColor(run.status) }}
                />
                <span className="text-sm font-medium">{triggerLabel(run.trigger, run.planFirst)}</span>
                {run.variantLabel && (
                  <span className="rounded-full bg-panel-2 px-2 py-0.5 text-[11px]">{run.variantLabel}</span>
                )}
                <span className="text-[11px] text-ink-dim">{statusLabel(run.status)}</span>
                <span className="ml-auto text-[11px] tabular-nums text-ink-dim">
                  {duration(run.seconds)} · {dollars(run.costUsd)}
                </span>
              </div>
              <div className="mt-1 text-[11px] text-ink-dim">
                {new Date(run.createdAt).toLocaleString()} · {run.engine}
                {run.model && <> · {run.model}</>}
                {run.agentName && <> · {run.agentName}</>}
              </div>
              {tokens && <div className="mt-0.5 text-[11px] tabular-nums text-ink-dim">{tokens}</div>}
              {excerpt && <p className="mt-1.5 text-xs text-ink">{excerpt}</p>}
              {(run.resumedFrom || run.rateLimitAttempts > 0) && (
                <div className="mt-0.5 text-[11px] text-ink-dim">
                  {run.resumedFrom && "Continued an earlier run's session"}
                  {run.resumedFrom && run.rateLimitAttempts > 0 && " · "}
                  {run.rateLimitAttempts > 0 &&
                    `waited out the rate limit ${run.rateLimitAttempts}×`}
                </div>
              )}
              {stopped && <RunError reason={stopped.text} tone={stopped.tone} compact className="mt-2" />}
              <div className="mt-2 flex flex-wrap gap-2">
                {!shown && (
                  <button
                    onClick={() => onView(run.runId)}
                    className="rounded-md border border-line px-2 py-0.5 text-[11px] hover:bg-panel"
                  >
                    View transcript
                  </button>
                )}
                {run.resumeCommand && (
                  <button
                    onClick={() => copy(run)}
                    title={run.resumeCommand}
                    className="rounded-md border border-line px-2 py-0.5 text-[11px] hover:bg-panel"
                  >
                    {copied === run.runId ? "Copied" : "Copy resume command"}
                  </button>
                )}
              </div>
            </li>
          );
        })}
      </ol>
      {runs.some((r) => r.resumeCommand) && (
        <p className="mt-3 text-[11px] text-ink-dim">
          The resume command continues that session in your own terminal, in the folder it ran in. It
          is offered only while nothing is running on this card.
        </p>
      )}
    </div>
  );
}
