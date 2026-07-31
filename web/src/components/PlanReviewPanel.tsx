import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, TaskPlan } from "../lib/api";
import { Markdown } from "./Markdown";

/**
 * The plan a card wrote before starting, waiting on you.
 *
 * Three answers, and the middle one is the reason this exists: approve it,
 * *rewrite* it and approve that, or send it back with a note. Editing beats
 * rejecting when the plan is 90% right — retyping the feedback and paying for
 * another planning pass to fix one line is worse than fixing the line.
 *
 * Rendered read-only once the run has moved on, because what was agreed is
 * still the most useful thing to read next to the diff.
 */
export function PlanReviewPanel({
  runId,
  onChanged,
}: {
  runId: string;
  onChanged: () => void;
}) {
  const [plan, setPlan] = useState<TaskPlan | null>(null);
  const [draft, setDraft] = useState<string | null>(null);
  const [note, setNote] = useState("");
  const [asking, setAsking] = useState(false);
  const [busy, setBusy] = useState<"approve" | "revise" | "save" | null>(null);
  const [error, setError] = useState<string | null>(null);
  /**
   * Runs this panel has already acted on.
   *
   * A Set keyed by run rather than a boolean, and that is not stylistic: the
   * task drawer is rendered with a constant key, so opening a different card is
   * a prop change, not a remount. A boolean set while looking at card A would
   * hide card B's plan, which is still genuinely waiting.
   */
  const [settled, setSettled] = useState<Set<string>>(new Set());
  /** Bumped after every action to re-read the plan the server now has. */
  const [reloads, setReloads] = useState(0);

  const load = useCallback(
    () => api.taskPlan(runId).catch(() => null),
    [runId],
  );

  useEffect(() => {
    let live = true;
    load().then((p) => live && p && setPlan(p));
    return () => {
      live = false;
    };
  }, [load, reloads]);

  if (!plan?.content) return null;

  const editing = draft !== null;
  const dirty = editing && draft.trim() !== plan.content.trim();
  // The server cannot tell this component to hide, and nothing else refetches
  // it. Without the local mask the amber panel stayed on screen after a
  // successful approve, offering a button whose second press was a guaranteed
  // 409 for a run that had already started.
  const awaiting = plan.awaitingApproval && !settled.has(runId);

  const act = async (
    kind: "approve" | "revise" | "save",
    fn: () => Promise<unknown>,
  ) => {
    setBusy(kind);
    setError(null);
    // Optimistic, and before the await deliberately: the panel goes away on the
    // click rather than whenever a round trip happens to land.
    setSettled((s) => new Set(s).add(runId));
    try {
      await fn();
      onChanged();
      // Leave the editor and the feedback box behind, so re-opening the card
      // shows the agreed plan rather than a half-typed draft of it.
      setDraft(null);
      setAsking(false);
      setNote("");
      // Deliberately does NOT clear `busy` — clearing it is what re-armed the
      // button. The panel is gone either way; the server's own answer arrives
      // through the reload below and takes over from the optimistic hide.
      setReloads((n) => n + 1);
    } catch (e) {
      setBusy(null);
      // Ask the server before deciding whether to put the panel back. A 409
      // means this run is genuinely no longer parked, so re-showing an Approve
      // button would only earn the same 409; a network failure means nothing
      // happened and the panel — and the error — must return.
      const fresh = await load();
      if (fresh) setPlan(fresh);
      if (!fresh || fresh.awaitingApproval) {
        setSettled((s) => {
          const next = new Set(s);
          next.delete(runId);
          return next;
        });
        setError(String(e).replace(/^Error:\s*/, ""));
      }
    }
  };

  // Saving an edit and approving are one gesture, not two: nobody edits a plan
  // in order to leave it sitting there.
  const approve = () =>
    act("approve", async () => {
      if (dirty) await api.saveTaskPlan(runId, draft!.trim());
      await api.approveTaskPlan(runId);
    });

  if (!awaiting) {
    return (
      <div className="border-b border-line px-5 py-3">
        <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          Approved plan{plan.edited ? " (edited)" : ""}
        </div>
        <div className="max-h-64 overflow-y-auto rounded-xl border border-line bg-panel-2 px-3 py-2 text-sm">
          <Markdown>{plan.content}</Markdown>
        </div>
      </div>
    );
  }

  return (
    <motion.div
      initial={{ opacity: 0, y: 4 }}
      animate={{ opacity: 1, y: 0 }}
      className="border-b border-line bg-amber-50/40 px-5 py-3"
    >
      <div className="mb-2 flex items-center justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-amber-800">
          Plan — nothing has changed yet
        </span>
        <button
          onClick={() => setDraft(editing ? null : plan.content!)}
          className="text-[11px] text-ink-dim hover:text-ink"
        >
          {editing ? "Cancel edit" : "Edit"}
        </button>
      </div>

      {editing ? (
        <textarea
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          rows={16}
          spellCheck={false}
          className="w-full resize-y rounded-xl border border-line bg-panel px-3 py-2 font-mono text-xs outline-none focus:border-accent"
        />
      ) : (
        <div className="max-h-80 overflow-y-auto rounded-xl border border-line bg-panel px-3 py-2 text-sm">
          <Markdown>{plan.content}</Markdown>
        </div>
      )}

      {error && (
        <div className="mt-2 rounded-lg bg-red-50 px-3 py-1.5 text-xs text-danger">
          {error}
        </div>
      )}

      {asking ? (
        <div className="mt-2">
          <textarea
            autoFocus
            value={note}
            onChange={(e) => setNote(e.target.value)}
            rows={3}
            placeholder="What's wrong with it? The next pass gets this plus the plan above."
            className="w-full resize-none rounded-xl border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
          />
          <div className="mt-2 flex gap-2">
            <motion.button
              whileTap={{ scale: 0.96 }}
              disabled={!note.trim() || !!busy}
              onClick={() =>
                act("revise", () => api.reviseTaskPlan(runId, note.trim()))
              }
              className="rounded-lg border border-line px-3.5 py-1.5 text-xs font-medium disabled:opacity-50"
            >
              {busy === "revise" ? "Sending…" : "Send it back"}
            </motion.button>
            <button
              onClick={() => setAsking(false)}
              className="text-xs text-ink-dim hover:text-ink"
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            disabled={!!busy || (editing && !draft.trim())}
            onClick={approve}
            className="rounded-lg bg-accent px-3.5 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy === "approve"
              ? "Starting…"
              : dirty
                ? "Save and start work"
                : "Approve and start work"}
          </motion.button>
          <button
            onClick={() => setAsking(true)}
            disabled={!!busy}
            className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:bg-panel-2 disabled:opacity-50"
          >
            Ask for changes
          </button>
          {dirty && (
            <span className="text-[11px] text-amber-800">
              your edits are saved when you start
            </span>
          )}
        </div>
      )}
    </motion.div>
  );
}
