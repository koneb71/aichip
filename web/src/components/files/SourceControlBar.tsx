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
 * it. Painted in the Files tab's IDE palette, the only place it appears.
 */
export function SourceControlBar({
  projectId,
  refreshKey = 0,
  onState,
}: {
  projectId: string;
  /** Bumped by the panel when a save lands, so the dirty count stays true. */
  refreshKey?: number;
  /** The panel's status bar shows the branch too — one fetch, not two. */
  onState?: (s: CheckoutState) => void;
}) {
  const [state, setState] = useState<CheckoutState | null>(null);
  const [busy, setBusy] = useState<"commit" | "pull" | "push" | null>(null);
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);
  const [committing, setCommitting] = useState(false);
  const [message, setMessage] = useState("");

  const refresh = useCallback(() => {
    api
      .projectCheckout(projectId)
      .then((s) => {
        setState(s);
        onState?.(s);
      })
      .catch(() => {});
    // eslint-disable-next-line react-hooks/exhaustive-deps
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

  const btn =
    "rounded border border-[#3c3c3c] px-2 py-0.5 text-[11px] text-[#cccccc] hover:bg-[#2a2d2e] disabled:opacity-40";

  return (
    <div className="border-b border-[#3c3c3c] px-3 py-1.5 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="flex items-center gap-1 font-mono text-[#cccccc]" title="Current branch">
          ⎇ {state.branch ?? "detached"}
        </span>
        {dirtyCount > 0 && (
          <span
            className="rounded-full bg-[#3a3100] px-2 py-0.5 text-[11px] text-[#e2c08d]"
            title={state.dirty.map((d) => d.path).join("\n")}
          >
            {dirtyCount} changed
          </span>
        )}
        {!unpublished && (state.behind ?? 0) > 0 && (
          <span className="text-[11px] text-[#8c8c8c]" title="Commits on the upstream you don't have">
            ↓{state.behind}
          </span>
        )}
        {!unpublished && (state.ahead ?? 0) > 0 && (
          <span className="text-[11px] text-[#8c8c8c]" title="Your commits the upstream doesn't have">
            ↑{state.ahead}
          </span>
        )}

        <span className="ml-auto flex items-center gap-1.5">
          {dirtyCount > 0 && !committing && (
            <button onClick={() => setCommitting(true)} disabled={busy !== null} className={btn}>
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
                className={btn}
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
                className={btn}
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
            className="min-w-0 flex-1 rounded border border-[#0e639c] bg-[#3c3c3c] px-2 py-1 text-[#cccccc] outline-none placeholder:text-[#8c8c8c]"
          />
          <button
            onClick={commit}
            disabled={!message.trim()}
            className="rounded bg-[#0e639c] px-2.5 py-1 text-white hover:bg-[#1177bb] disabled:opacity-40"
          >
            Commit
          </button>
        </div>
      )}

      {notice && (
        <button
          onClick={() => setNotice(null)}
          title="Dismiss"
          className={`mt-1.5 block w-full rounded px-2 py-1 text-left text-[11px] ${
            notice.kind === "ok" ? "bg-[#2a2d2e] text-[#8c8c8c]" : "bg-[#5a1d1d]/40 text-[#f48771]"
          }`}
        >
          {notice.text}
        </button>
      )}
    </div>
  );
}
