import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";
import { Button } from "./ui/Button";
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
          <p className="text-fg-muted">
            Resolving conflicts with {base} in {merging.join(", ")}…
          </p>
        ) : (
          <div className="flex flex-wrap items-center gap-2 rounded-md bg-warning-subtle px-2 py-1.5 text-warning-fg">
            <span>
              Conflict markers remain in {merging.join(", ")}. Fix them in the Files tab, or
            </span>
            <Button variant="secondary" size="xs" onClick={update} disabled={acting}>
              {acting ? "Starting…" : "ask the agent again"}
            </Button>
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
        <span className="text-fg-muted">
          {status.behind === 0
            ? `Up to date with ${base}.`
            : `${status.behind} ${status.behind === 1 ? "commit" : "commits"} behind ${base}.`}
          {conflicted && " Merging conflicts with it — bring it in here and an agent resolves the conflict on this card's branch."}
        </span>
        {status.behind > 0 && (
          <Button
            variant={conflicted ? "primary" : "secondary"}
            size="xs"
            onClick={update}
            disabled={busy || acting}
            className="ml-auto"
          >
            {acting ? "Updating…" : `Update from ${base}`}
          </Button>
        )}
      </div>
      {note && <p className="mt-1 text-fg-muted">{note}</p>}
      {error && <RunError reason={error} className="mt-2" />}
    </div>
  );
}
