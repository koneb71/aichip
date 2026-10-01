import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { RunError } from "./ui/RunError";

/**
 * How the card's branch stands against the base, and the way out of a conflict.
 *
 * A merge refused as a conflict used to be a dead end — "resolve the conflict
 * on the branch itself", in a terminal. Now the base comes into the card's own
 * branch, in its worktree, and an agent resolves what conflicts there, in the
 * same diff a person then reads.
 */
export function BaseStatus({
  taskId,
  busy,
  refreshKey,
  conflicted,
  onChanged,
}: {
  taskId: string;
  /** An agent is working on the card — including one resolving conflicts. */
  busy: boolean;
  refreshKey: string;
  /** Merge was just refused as a conflict: say what to do about it. */
  conflicted: boolean;
  onChanged: () => void;
}) {
  const [status, setStatus] = useState<{ base?: string; behind: number | null; merging: string[] | null } | null>(
    null,
  );
  const [acting, setActing] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api.baseStatus(taskId).then(setStatus).catch(() => {});
  }, [taskId]);
  useEffect(load, [load, refreshKey, conflicted]);

  if (!status || status.behind === null) return null;
  const base = status.base ?? "the base branch";
  const merging = status.merging;

  const update = async () => {
    setActing(true);
    setError(null);
    setNote(null);
    try {
      const r = await api.updateFromBase(taskId);
      setNote(
        r.outcome === "up_to_date"
          ? `Already up to date with ${base}.`
          : r.outcome === "merged"
            ? `Brought ${base} in. The diff shows only this card's work against it.`
            : `${r.files?.length ?? 0} ${r.files?.length === 1 ? "file conflicts" : "files conflict"} — an agent is resolving ${r.files?.length === 1 ? "it" : "them"} in this card's worktree.`,
      );
      load();
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setActing(false);
    }
  };

  if (merging && merging.length > 0) {
    return (
      <div className="text-xs">
        {busy ? (
          <p className="text-ink-dim">
            Resolving conflicts with {base} in {merging.join(", ")}…
          </p>
        ) : (
          <div className="flex flex-wrap items-center gap-2 rounded-md bg-amber-50 px-2 py-1.5 text-amber-800">
            <span>
              Conflict markers remain in {merging.join(", ")}. Fix them in the Files tab, or
            </span>
            <button
              onClick={update}
              disabled={acting}
              className="rounded-md border border-amber-300 px-2 py-0.5 text-[11px] hover:bg-amber-100 disabled:opacity-40"
            >
              {acting ? "Starting…" : "ask the agent again"}
            </button>
          </div>
        )}
        {error && <RunError reason={error} className="mt-2" />}
      </div>
    );
  }

  if (status.behind === 0 && !conflicted && !note) return null;

  return (
    <div className="text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="text-ink-dim">
          {status.behind === 0
            ? `Up to date with ${base}.`
            : `${status.behind} ${status.behind === 1 ? "commit" : "commits"} behind ${base}.`}
          {conflicted && " Merging conflicts with it — bring it in here and an agent resolves the conflict on this card's branch."}
        </span>
        {status.behind > 0 && (
          <button
            onClick={update}
            disabled={busy || acting}
            className={`ml-auto rounded-md px-2 py-0.5 text-[11px] disabled:opacity-40 ${
              conflicted ? "bg-accent font-medium text-white" : "border border-line hover:bg-panel-2"
            }`}
          >
            {acting ? "Updating…" : `Update from ${base}`}
          </button>
        )}
      </div>
      {note && <p className="mt-1 text-ink-dim">{note}</p>}
      {error && <RunError reason={error} className="mt-2" />}
    </div>
  );
}
