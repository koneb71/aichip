import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { motion } from "framer-motion";
import { api, type AppBuild } from "../../lib/api";
import { buildLine } from "../../lib/apps";

/**
 * What has been asked of this app, and the way back.
 *
 * `revertible` is the server's answer rather than this component's: which build
 * may be undone is a rule about what `base_commit` can promise, and deciding it
 * twice is how the browser comes to offer an undo that throws away a later
 * change.
 */
export function BuildHistory({
  appId,
  projectId,
  onChanged,
}: {
  appId: string;
  /** The app's own project, which is where its cards live. */
  projectId: string;
  onChanged: () => void;
}) {
  const [builds, setBuilds] = useState<AppBuild[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () => api.appBuilds(appId).then((r) => setBuilds(r.builds)).catch(() => {}),
    [appId],
  );

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Only while a card is actually working. A settled history has nothing
  // further to say, and the landing happens in the orchestrator rather than
  // here, so polling is the only way this page learns it happened.
  const running = builds.some((b) => b.status === "running");
  useEffect(() => {
    if (!running) return;
    const t = setInterval(() => {
      refresh();
      onChanged();
    }, 3000);
    return () => clearInterval(t);
  }, [running, refresh, onChanged]);

  const revert = async (build: AppBuild) => {
    if (
      !window.confirm(
        `Undo "${build.brief}"?\n\nThis puts the app's folder back exactly as it was before ` +
          `that change, so anything edited in the Files tab since then is lost. ` +
          `A column the change added is a column this removes — that part waits for you ` +
          `to approve the SQL.`,
      )
    ) {
      return;
    }
    setBusy(true);
    setError(null);
    try {
      await api.revertAppBuild(appId, build.id);
      await refresh();
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  if (builds.length === 0) return null;

  return (
    // Bounded and scrolling, not flexible: this sits under a view that wants
    // every pixel it can have, and a long history must not squeeze it away.
    <div className="mt-6 max-h-56 shrink-0 overflow-y-auto">
      <div className="mb-2 text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
        Changes
      </div>
      {error && (
        <div className="mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}
      <div className="flex flex-col gap-1">
        {builds.map((b) => (
          <div
            key={b.id}
            className="flex items-center gap-3 rounded-lg border border-line px-3 py-2 text-xs"
          >
            <span
              className={
                "h-1.5 w-1.5 shrink-0 rounded-full " +
                (b.status === "running"
                  ? "animate-pulse bg-tier-medium"
                  : b.status === "landed"
                    ? b.error
                      ? "bg-amber-500"
                      : "bg-accent"
                    : b.status === "reverted"
                      ? "bg-line"
                      : "bg-danger")
              }
            />
            <div className="min-w-0 flex-1">
              <div className="truncate">{b.brief}</div>
              <div className="truncate text-[11px] text-ink-dim">{buildLine(b)}</div>
              {b.error && (
                <pre className="mt-1 max-h-24 overflow-auto whitespace-pre-wrap font-mono text-[10px] text-danger">
                  {b.error}
                </pre>
              )}
            </div>
            {b.taskId && (
              <Link
                to={`/projects/${projectId}?task=${b.taskId}`}
                title="Open the card, where the run's output and diff are."
                className="shrink-0 text-ink-dim hover:text-ink hover:underline"
              >
                Card
              </Link>
            )}
            {b.revertible && (
              <motion.button
                whileTap={{ scale: 0.96 }}
                onClick={() => revert(b)}
                disabled={busy}
                title="Put the app back exactly as it was before this change."
                className="shrink-0 rounded-lg border border-line px-2 py-1 hover:bg-line/40 disabled:opacity-50"
              >
                Undo
              </motion.button>
            )}
          </div>
        ))}
      </div>
    </div>
  );
}
