import { useCallback, useEffect, useState } from "react";
import { api, PreviewRecipe } from "../lib/api";
import { Button } from "./ui/Button";
import { Textarea } from "./ui/Field";

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
        <Button size="sm" onClick={propose} disabled={busy}>
          {busy ? "Reading the project…" : "Write one for me"}
        </Button>
        <div className="mt-1 text-[11px] text-fg-muted">
          An agent reads this project and decides what it needs — a Dockerfile if
          one container will do, a compose stack if it genuinely won't. You read
          it before anything is built.
        </div>
        {error && (
          <div className="mt-1 rounded-lg bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
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
        <span className="rounded-md bg-border/60 px-1.5 py-0.5 uppercase tracking-wide text-fg-muted">
          {recipe.kind === "compose" ? "compose stack" : "dockerfile"}
        </span>
        <span className="text-fg-muted">
          {recipe.kind === "compose"
            ? "The agent decided one container isn't enough. Its declared host ports are ignored — the preview publishes one, on loopback."
            : "The agent decided one container serves this."}
        </span>
      </div>
      {!approved && (
        <div className="rounded-lg bg-warning-subtle px-2.5 py-1.5 text-[11px] text-warning-fg">
          <span className="font-semibold">An agent wrote this.</span> Its build
          steps execute on this machine. Read it, change anything you like, then
          approve.
        </div>
      )}
      <Textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        spellCheck={false}
        rows={Math.min(18, Math.max(6, draft.split("\n").length))}
        className="resize-y py-2! font-mono text-[11px]!"
      />
      <div className="flex flex-wrap items-center gap-2">
        <Button variant="primary" size="sm" onClick={approve} disabled={busy || (approved && !changed)}>
          {approved ? (changed ? "Approve changes" : "Approved") : "Approve & use"}
        </Button>
        <Button size="sm" onClick={propose} disabled={busy}>
          Ask again
        </Button>
        {approved && !changed && recipe.edited && (
          <span className="text-[11px] text-fg-muted">you rewrote this one</span>
        )}
      </div>
      {error && (
        <div className="rounded-lg bg-danger-subtle px-2.5 py-1.5 text-[11px] text-danger-fg">
          {error}
        </div>
      )}
    </div>
  );
}
