import { useCallback, useEffect, useState } from "react";
import { api, CheckoutState } from "../../lib/api";
import { Button } from "../ui/Button";
import { Input } from "../ui/Field";

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
 * it. Painted in the app's tokens, like the Files tab that hosts it.
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

  return (
    <div className="border-b border-border px-3 py-1.5 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="flex items-center gap-1 font-mono text-fg" title="Current branch">
          ⎇ {state.branch ?? "detached"}
        </span>
        {dirtyCount > 0 && (
          <span
            className="rounded-full bg-warning-subtle px-2 py-0.5 text-[11px] text-warning-fg"
            title={state.dirty.map((d) => d.path).join("\n")}
          >
            {dirtyCount} changed
          </span>
        )}
        {!unpublished && (state.behind ?? 0) > 0 && (
          <span className="text-[11px] text-fg-muted" title="Commits on the upstream you don't have">
            ↓{state.behind}
          </span>
        )}
        {!unpublished && (state.ahead ?? 0) > 0 && (
          <span className="text-[11px] text-fg-muted" title="Your commits the upstream doesn't have">
            ↑{state.ahead}
          </span>
        )}

        <span className="ml-auto flex items-center gap-1.5">
          {dirtyCount > 0 && !committing && (
            <Button size="xs" variant="secondary" onClick={() => setCommitting(true)} disabled={busy !== null}>
              {busy === "commit" ? "Committing…" : "Commit…"}
            </Button>
          )}
          {state.hasRemote && (
            <>
              {/* Titles on wrappers: a disabled kit Button takes no pointer
                  events, and the title is what says why it is disabled. */}
              <span
                className="shrink-0"
                title={
                  dirtyCount > 0
                    ? "Commit your changes first — pulling over an edited tree is how work gets tangled"
                    : "Fast-forward from the upstream"
                }
              >
                <Button
                  size="xs"
                  variant="secondary"
                  onClick={() => run("pull", () => api.pullCheckout(projectId), "up to date")}
                  disabled={busy !== null || dirtyCount > 0}
                >
                  {busy === "pull" ? "Pulling…" : "↓ Pull"}
                </Button>
              </span>
              <span
                className="shrink-0"
                title={
                  unpublished
                    ? "This branch has never been pushed — this publishes it"
                    : (state.ahead ?? 0) === 0
                      ? "Nothing to push"
                      : "Push your commits to the upstream"
                }
              >
                <Button
                  size="xs"
                  variant="secondary"
                  onClick={() => run("push", () => api.pushCheckout(projectId), "pushed")}
                  disabled={busy !== null || (!unpublished && (state.ahead ?? 0) === 0)}
                >
                  {busy === "push" ? "Pushing…" : unpublished ? "↑ Publish" : "↑ Push"}
                </Button>
              </span>
            </>
          )}
        </span>
      </div>

      {committing && (
        <div className="mt-1.5 flex items-center gap-1.5">
          <Input
            autoFocus
            value={message}
            onChange={(e) => setMessage(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") commit();
              if (e.key === "Escape") setCommitting(false);
            }}
            placeholder="Commit message…"
            className="min-w-0 flex-1"
          />
          <Button size="sm" variant="primary" onClick={commit} disabled={!message.trim()}>
            Commit
          </Button>
        </div>
      )}

      {notice && (
        <button
          onClick={() => setNotice(null)}
          title="Dismiss"
          className={`mt-1.5 block w-full rounded px-2 py-1 text-left text-[11px] ${
            notice.kind === "ok" ? "bg-panel text-fg-muted" : "bg-danger-subtle text-danger-fg"
          }`}
        >
          {notice.text}
        </button>
      )}
    </div>
  );
}
