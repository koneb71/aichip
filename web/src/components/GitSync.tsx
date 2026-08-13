import { useCallback, useEffect, useState } from "react";
import { api, CheckoutState } from "../lib/api";

/**
 * The project header's git corner: where the checkout stands against its
 * upstream, and the two verbs that move it — pull and push — one click from
 * any tab instead of buried inside Files.
 *
 * Same honesty rules as the Files tab's source-control bar, because they are
 * the same operations: pull is fast-forward only and git's own refusal comes
 * back verbatim; push on a never-published branch publishes it; pulling over
 * an edited tree is refused with the reason. Committing stays in the Files
 * tab next to the editor that made the changes — the dirty chip here is a
 * door to it, not a substitute.
 */
export function GitSync({
  projectId,
  onOpenFiles,
}: {
  projectId: string;
  /** The dirty chip opens the Files tab, where the Commit button lives. */
  onOpenFiles: () => void;
}) {
  const [state, setState] = useState<CheckoutState | null>(null);
  const [busy, setBusy] = useState<"pull" | "push" | null>(null);
  const [notice, setNotice] = useState<{ kind: "ok" | "err"; text: string } | null>(null);

  const refresh = useCallback(() => {
    api
      .projectCheckout(projectId)
      .then(setState)
      .catch(() => setState(null));
  }, [projectId]);

  useEffect(() => {
    refresh();
    // Coming back from a terminal or another app is exactly when the counts
    // are most likely stale — a fetch on focus keeps them true for free.
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, [refresh]);

  // Not a git checkout (a space, a folder, a repo that lost its .git):
  // nothing here could work, so nothing is shown.
  if (!state?.vcs || !state.branch) return null;

  const run = async (verb: "pull" | "push", go: () => Promise<unknown>, ok: string) => {
    setBusy(verb);
    setNotice(null);
    try {
      const r = (await go()) as { detail?: string };
      setNotice({ kind: "ok", text: r.detail?.split("\n")[0] || ok });
    } catch (e) {
      setNotice({ kind: "err", text: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(null);
      refresh();
    }
  };

  const dirty = state.dirty.length;
  const unpublished = state.ahead == null;
  const behind = state.behind ?? 0;
  const ahead = state.ahead ?? 0;
  const btn =
    "shrink-0 rounded-full border border-line px-2 py-0.5 text-[11px] text-ink-dim transition-colors hover:border-accent hover:text-accent disabled:opacity-40 disabled:hover:border-line disabled:hover:text-ink-dim";

  return (
    <span className="flex min-w-0 items-center gap-1.5">
      <span className="shrink-0 font-mono text-[11px] text-ink-dim" title="Current branch">
        ⎇ {state.branch}
      </span>
      {dirty > 0 && (
        <button
          onClick={onOpenFiles}
          title={`${state.dirty.map((d) => d.path).join("\n")}\n\nOpen Files to commit`}
          className="shrink-0 rounded-full bg-amber-50 px-2 py-0.5 text-[11px] text-amber-700 hover:bg-amber-100"
        >
          {dirty} uncommitted
        </button>
      )}
      {state.hasRemote && (
        <>
          <button
            onClick={() => run("pull", () => api.pullCheckout(projectId), "up to date")}
            disabled={busy !== null || dirty > 0}
            title={
              dirty > 0
                ? "Commit your changes first — pulling over an edited tree is how work gets tangled"
                : behind > 0
                  ? `Fast-forward ${behind} commit${behind === 1 ? "" : "s"} from the upstream`
                  : "Fast-forward from the upstream"
            }
            className={btn}
          >
            {busy === "pull" ? "Pulling…" : `↓ Pull${behind > 0 ? ` ${behind}` : ""}`}
          </button>
          <button
            onClick={() => run("push", () => api.pushCheckout(projectId), "pushed")}
            disabled={busy !== null || (!unpublished && ahead === 0)}
            title={
              unpublished
                ? "This branch has never been pushed — this publishes it"
                : ahead === 0
                  ? "Nothing to push"
                  : `Push ${ahead} commit${ahead === 1 ? "" : "s"} to the upstream`
            }
            className={btn}
          >
            {busy === "push"
              ? "Pushing…"
              : unpublished
                ? "↑ Publish"
                : `↑ Push${ahead > 0 ? ` ${ahead}` : ""}`}
          </button>
        </>
      )}
      {notice && (
        <button
          onClick={() => setNotice(null)}
          title={notice.text}
          className={`min-w-0 max-w-[200px] truncate text-[11px] ${
            notice.kind === "ok" ? "text-ink-dim" : "text-danger"
          }`}
        >
          {notice.text}
        </button>
      )}
    </span>
  );
}
