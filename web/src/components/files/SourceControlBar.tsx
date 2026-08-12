import { useCallback, useEffect, useState } from "react";
import { api, CheckoutState } from "../../lib/api";

/**
 * The editor's git corner: where the checkout stands, and the three verbs a
 * person editing files actually reaches for — commit, pull, push.
 *
 * Everything stays honest about what a dashboard button may decide. Pull is
 * fast-forward only: whose history wins on a diverged branch is not a
 * button's decision, so git's own refusal comes back verbatim. Push on a
 * never-published branch publishes it, because that is the only thing "push
 * this" can mean there.
 *
 * Shown only for the project checkout — a card's worktree already has its own
 * lifecycle (review, merge, PR), and offering push there would route around
 * it.
 */
export function SourceControlBar({
  projectId,
  refreshKey = 0,
}: {
  projectId: string;
  /** Bumped by the panel when a save lands, so the dirty count stays true. */
  refreshKey?: number;
}) {
  const [state, setState] = useState<CheckoutState | null>(null);
  const [busy, setBusy] = useState<"commit" | "pull" | "push" | null>(null);
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [committing, setCommitting] = useState(false);
  const [message, setMessage] = useState("");

  const refresh = useCallback(() => {
    api
      .projectCheckout(projectId)
      .then(setState)
      .catch(() => {});
  }, [projectId]);

  useEffect(() => {
    setNotice(null);
    setCommitting(false);
    refresh();
  }, [refresh, refreshKey]);

  if (!state?.vcs) return null;

  const run = async (
    verb: "commit" | "pull" | "push",
    go: () => Promise<unknown>,
    ok: string,
  ) => {
    setBusy(verb);
    setNotice(null);
    try {
      const r = (await go()) as { detail?: string };
      setNotice({ kind: "ok", text: r.detail?.split("\n")[0] || ok });
      refresh();
    } catch (e) {
      // git's own words: "not possible to fast-forward" beats anything we
      // could paraphrase it into.
      setNotice({ kind: "err", text: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(null);
    }
  };

  const commit = () => {
    const m = message.trim();
    if (!m) return;
    setCommitting(false);
    setMessage("");
    run("commit", () => api.commitCheckout(projectId, m), "committed");
  };

  const dirtyCount = state.dirty.length;
  const unpublished = state.ahead == null;

  return (
    <div className="border-b border-line bg-panel px-3 py-1.5 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="flex items-center gap-1 font-mono text-ink-dim" title="Current branch">
          ⎇ {state.branch ?? "detached"}
        </span>
        {dirtyCount > 0 && (
          <span
            className="rounded-full bg-amber-100 px-2 py-0.5 text-[11px] text-amber-800"
            title={state.dirty.map((d) => d.path).join("\n")}
          >
            {dirtyCount} changed
          </span>
        )}
        {!unpublished && (state.behind ?? 0) > 0 && (
          <span className="text-[11px] text-ink-dim" title="Commits on the upstream you don't have">
            ↓{state.behind}
          </span>
        )}
        {!unpublished && (state.ahead ?? 0) > 0 && (
          <span className="text-[11px] text-ink-dim" title="Your commits the upstream doesn't have">
            ↑{state.ahead}
          </span>
        )}

        <span className="ml-auto flex items-center gap-1.5">
          {dirtyCount > 0 && !committing && (
            <button
              onClick={() => setCommitting(true)}
              disabled={busy !== null}
              className="rounded-lg border border-line px-2.5 py-1 hover:border-ink-dim disabled:opacity-50"
            >
              {busy === "commit" ? "Committing…" : "Commit…"}
            </button>
          )}
          {state.hasRemote && (
            <>
              <button
                onClick={() => run("pull", () => api.pullCheckout(projectId), "up to date")}
                disabled={busy !== null || dirtyCount > 0}
                title={
                  dirtyCount > 0
                    ? "Commit your changes first — pulling over an edited tree is how work gets tangled"
                    : "Fast-forward from the upstream"
                }
                className="rounded-lg border border-line px-2.5 py-1 hover:border-ink-dim disabled:opacity-50"
              >
                {busy === "pull" ? "Pulling…" : "↓ Pull"}
              </button>
              <button
                onClick={() => run("push", () => api.pushCheckout(projectId), "pushed")}
                disabled={busy !== null || (!unpublished && (state.ahead ?? 0) === 0)}
                title={
                  unpublished
                    ? "This branch has never been pushed — this publishes it"
                    : (state.ahead ?? 0) === 0
                      ? "Nothing to push"
                      : "Push your commits to the upstream"
                }
                className="rounded-lg border border-line px-2.5 py-1 hover:border-ink-dim disabled:opacity-50"
              >
                {busy === "push" ? "Pushing…" : unpublished ? "↑ Publish" : "↑ Push"}
              </button>
            </>
          )}
        </span>
      </div>

      {committing && (
        <div className="mt-1.5 flex items-center gap-1.5">
          <input
            autoFocus
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setCommitting(false);
            }}
            placeholder="Commit message…"
            className="min-w-0 flex-1 rounded-lg border border-accent bg-panel px-2 py-1 outline-none"
          />
          <button
            onClick={commit}
            disabled={!message.trim()}
            className="rounded-lg bg-accent px-2.5 py-1 text-white disabled:opacity-40"
          >
            Commit
          </button>
        </div>
      )}

      {notice && (
        <button
          onClick={() => setNotice(null)}
          title="Dismiss"
          className={`mt-1.5 block w-full rounded-lg px-2 py-1 text-left text-[11px] ${
            notice.kind === "ok" ? "bg-panel-2 text-ink-dim" : "bg-red-50 text-danger"
          }`}
        >
          {notice.text}
        </button>
      )}
    </div>
  );
}
