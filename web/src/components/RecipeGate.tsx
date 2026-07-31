import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, PreviewRecipe } from "../lib/api";

/**
 * What an agent decided this project needs, shown in full before it is built.
 *
 * The agent chooses: a Dockerfile when one container serves the whole thing, a
 * compose file when the project cannot start without a database or a separate
 * API. Which it picked is worth showing — a stack takes longer to build and
 * runs more than what you opened.
 *
 * The gate is the feature. Neither file is configuration: `RUN` executes
 * arbitrary commands on this machine while the image is built, and compose can
 * ask for volumes and services besides. So the whole text is shown, editable,
 * and nothing runs until someone presses the button that says so.
 *
 * Editing and approving are one action deliberately — approving "the current
 * proposal" by reference would leave a window where the text that gets built
 * is not the text that was read.
 */
export function RecipeGate({
  projectId,
  onApproved,
}: {
  projectId: string;
  onApproved: () => void;
}) {
  const [recipe, setRecipe] = useState<PreviewRecipe | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .previewRecipe(projectId)
        .then((r) => {
          setRecipe(r.recipe);
          if (r.recipe) setDraft(r.recipe.dockerfile);
        })
        .catch(() => {}),
    [projectId],
  );

  useEffect(() => {
    refresh();
  }, [refresh]);

  const propose = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.proposeRecipe(projectId);
      setRecipe(r.recipe);
      setDraft(r.recipe.dockerfile);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const approve = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.approveRecipe(projectId, draft);
      await refresh();
      onApproved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  if (!recipe) {
    return (
      <div className="mt-1.5">
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={propose}
          disabled={busy}
          className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-50"
        >
          {busy ? "Reading the project…" : "Write one for me"}
        </motion.button>
        <div className="mt-1 text-[11px] text-ink-dim">
          An agent reads this project and decides what it needs — a Dockerfile if
          one container will do, a compose stack if it genuinely won't. You read
          it before anything is built.
        </div>
        {error && (
          <div className="mt-1 rounded-lg bg-red-50 px-2.5 py-1.5 text-[11px] text-danger">
            {error}
          </div>
        )}
      </div>
    );
  }

  const approved = recipe.status === "approved";
  const changed = draft.trim() !== recipe.dockerfile.trim();

  return (
    <div className="mt-1.5 space-y-1.5">
      <div className="flex flex-wrap items-baseline gap-2 text-[11px]">
        <span className="rounded-md bg-line/60 px-1.5 py-0.5 uppercase tracking-wide text-ink-dim">
          {recipe.kind === "compose" ? "compose stack" : "dockerfile"}
        </span>
        <span className="text-ink-dim">
          {recipe.kind === "compose"
            ? "The agent decided one container isn't enough. Its declared host ports are ignored — the preview publishes one, on loopback."
            : "The agent decided one container serves this."}
        </span>
      </div>
      {!approved && (
        <div className="rounded-lg bg-amber-50 px-2.5 py-1.5 text-[11px] text-amber-900">
          <span className="font-semibold">An agent wrote this.</span> Its build
          steps execute on this machine. Read it, change anything you like, then
          approve.
        </div>
      )}
      <textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        spellCheck={false}
        rows={Math.min(18, Math.max(6, draft.split("\n").length))}
        className="w-full resize-y rounded-lg border border-line bg-panel px-2.5 py-2 font-mono text-[11px] leading-relaxed outline-none focus:border-accent"
      />
      <div className="flex flex-wrap items-center gap-2">
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={approve}
          disabled={busy || (approved && !changed)}
          className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white disabled:opacity-40"
        >
          {approved ? (changed ? "Approve changes" : "Approved") : "Approve & use"}
        </motion.button>
        <button
          onClick={propose}
          disabled={busy}
          className="rounded-lg border border-line px-2.5 py-1 text-xs hover:bg-line/40 disabled:opacity-50"
        >
          Ask again
        </button>
        {approved && !changed && recipe.edited && (
          <span className="text-[11px] text-ink-dim">you rewrote this one</span>
        )}
      </div>
      {error && (
        <div className="rounded-lg bg-red-50 px-2.5 py-1.5 text-[11px] text-danger">
          {error}
        </div>
      )}
    </div>
  );
}
