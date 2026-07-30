import { useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, OrgAssignment, OrgRunDetail } from "../../lib/api";

/**
 * The plan, before anyone starts on it.
 *
 * Shown while a run is parked at `awaiting_approval`. Every edit writes
 * straight through to the assignment row, so what you approve is exactly
 * what the specialists receive.
 */
export function PlanReview({
  run,
  onChanged,
  onDecided,
  onFresh,
}: {
  run: OrgRunDetail;
  onChanged: () => void;
  /** Hide this panel now, before the poll catches up. */
  onDecided: () => void;
  /** Hand the parent the state the server returned, so no poll can undo it. */
  onFresh: (fresh: OrgRunDetail) => void;
}) {
  const [busy, setBusy] = useState<"approve" | "reject" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const assignments = run.assignments.filter(
    (a) => a.kind === "assignment" && a.status === "queued",
  );
  const specialists = run.roster.filter((m) => !m.isManager);

  const act = async (what: "approve" | "reject") => {
    setBusy(what);
    setError(null);
    try {
      const fresh =
        what === "approve"
          ? await api.approvePlan(run.id)
          : await api.rejectPlan(run.id, "you rejected the plan");
      // The endpoint hands back the run it just changed, so the panel goes as
      // soon as the request lands rather than on whichever of the 1500ms polls
      // happens to come next — and `onFresh` makes any poll already in flight
      // stale, which is what used to flip this panel back a second later.
      //
      // Both after the await, not before: the parent unmounts this panel the
      // moment it is told, taking the error box below with it, so a failure has
      // to be able to keep it on screen.
      onDecided();
      onFresh(fresh);
      onChanged();
      // `busy` is deliberately left set. Clearing it is what made the button
      // flip back to "Approve & start" and become clickable again while the
      // panel was still up — the UI visibly undoing the click.
    } catch (e) {
      setBusy(null);
      setError(String(e).replace(/^Error:\s*/, ""));
      // Still refresh: if this failed because the run moved on, the status alone
      // takes the panel down and no mask is needed.
      onChanged();
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: -6 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, y: -6 }}
      className="flex min-h-0 flex-col gap-2"
    >
      <div className="rounded-xl border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-800">
        <div className="font-medium">The plan is ready for you</div>
        <div className="mt-0.5">
          Nobody has started. Edit anything below, then approve — or cancel the run.
        </div>
      </div>

      <AnimatePresence initial={false}>
        {assignments.map((assignment, index) => (
          <PlanRow
            key={assignment.id}
            runId={run.id}
            assignment={assignment}
            index={index}
            specialists={specialists.map((m) => m.name)}
            color={
              run.roster.find((m) => m.name === assignment.assignee)?.color ??
              "var(--color-ink-dim)"
            }
            onChanged={onChanged}
          />
        ))}
      </AnimatePresence>

      {assignments.length === 0 && (
        <div className="rounded-xl border border-dashed border-line p-4 text-center text-xs text-ink-dim">
          Every assignment was removed. Approving now would do nothing.
        </div>
      )}

      {error && (
        <div className="rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      <div className="sticky bottom-0 flex items-center gap-2 border-t border-line bg-panel pt-2">
        <span className="text-[11px] text-ink-dim">
          {assignments.length} assignment{assignments.length === 1 ? "" : "s"} ·{" "}
          {new Set(assignments.map((a) => a.assignee)).size} on it
        </span>
        <button
          onClick={() => act("reject")}
          disabled={!!busy}
          className="ml-auto rounded-lg border border-line px-3 py-1.5 text-xs hover:border-danger hover:text-danger"
        >
          Cancel run
        </button>
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={() => act("approve")}
          disabled={!!busy || assignments.length === 0}
          className="rounded-lg bg-accent px-3.5 py-1.5 text-xs font-medium text-white disabled:opacity-50"
        >
          {busy === "approve" ? "Starting…" : "Approve & start"}
        </motion.button>
      </div>
    </motion.div>
  );
}

function PlanRow({
  runId,
  assignment,
  index,
  specialists,
  color,
  onChanged,
}: {
  runId: string;
  assignment: OrgAssignment;
  index: number;
  specialists: string[];
  color: string;
  onChanged: () => void;
}) {
  const [title, setTitle] = useState(assignment.title ?? "");
  const [brief, setBrief] = useState(assignment.brief ?? "");
  const [open, setOpen] = useState(false);

  // Written on blur rather than per keystroke: the poll would otherwise
  // overwrite the field mid-sentence.
  const save = async (body: Record<string, unknown>) => {
    try {
      await api.updateAssignment(runId, assignment.id, body);
      onChanged();
    } catch {
      /* the next poll restores the server's copy */
    }
  };

  return (
    <motion.div
      layout
      initial={{ opacity: 0, y: 8 }}
      animate={{ opacity: 1, y: 0 }}
      exit={{ opacity: 0, height: 0 }}
      className="rounded-xl border border-line bg-panel p-2.5"
    >
      <div className="flex items-center gap-2">
        <span className="text-[11px] text-ink-dim">{index + 1}</span>
        <input
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          onBlur={() => title !== assignment.title && save({ title })}
          className="min-w-0 flex-1 rounded border border-transparent px-1 py-0.5 text-xs font-medium outline-none hover:border-line focus:border-accent"
        />
        <button
          onClick={() => api.dropAssignment(runId, assignment.id).then(onChanged)}
          title="Remove this assignment"
          className="text-ink-dim hover:text-danger"
        >
          ✕
        </button>
      </div>

      <div className="mt-1.5 flex items-center gap-1.5">
        <select
          value={assignment.assignee ?? ""}
          onChange={(e) => save({ assignee: e.target.value })}
          className="rounded-full px-1.5 py-0.5 text-[10px] text-white"
          style={{ background: color }}
        >
          {specialists.map((name) => (
            <option key={name} value={name} className="bg-panel text-ink">
              {name}
            </option>
          ))}
        </select>
        {assignment.size && (
          <span className="rounded-full bg-panel-2 px-1.5 py-0.5 text-[10px] text-ink-dim">
            {assignment.size}
          </span>
        )}
        {assignment.dependsOn.length > 0 && (
          <span className="rounded-full bg-panel-2 px-1.5 py-0.5 font-mono text-[10px] text-ink-dim">
            after {assignment.dependsOn.join(", ")}
          </span>
        )}
        <button
          onClick={() => setOpen((o) => !o)}
          className="ml-auto text-[10px] text-ink-dim hover:text-ink"
        >
          {open ? "hide brief" : "brief"}
        </button>
      </div>

      <AnimatePresence>
        {open && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden"
          >
            <textarea
              value={brief}
              onChange={(e) => setBrief(e.target.value)}
              onBlur={() => brief !== assignment.brief && save({ brief })}
              rows={5}
              className="mt-2 w-full resize-none rounded-lg border border-line px-2 py-1.5 text-[11px] outline-none focus:border-accent"
            />
            {assignment.doneWhen.length > 0 && (
              <ul className="mt-1.5 space-y-0.5 text-[10px] text-ink-dim">
                {assignment.doneWhen.map((d, i) => (
                  <li key={i}>✓ {d}</li>
                ))}
              </ul>
            )}
          </motion.div>
        )}
      </AnimatePresence>
    </motion.div>
  );
}
