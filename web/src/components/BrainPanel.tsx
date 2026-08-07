import { useCallback, useEffect, useRef, useState } from "react";
import { api, ProjectBrain } from "../lib/api";

/**
 * What every run in this project should already know.
 *
 * The paragraph you would otherwise retype into every card — where the code
 * lives, how it is deployed, what not to touch. It reaches every board run,
 * every chat and every @-mention reply in this project, without being attached
 * to anything.
 *
 * Two controls that look small and are the whole safety story:
 *
 * - **Off, not deleted.** A brain that is steering runs wrongly is diagnosed by
 *   turning it off and retrying a plain run. Deleting it to test would destroy
 *   the thing being tested.
 * - **Saves carry the version they started from.** Two tabs open on this is the
 *   ordinary case — the second save is refused rather than silently winning.
 */
export function BrainPanel({ projectId }: { projectId: string }) {
  const [brain, setBrain] = useState<ProjectBrain | null>(null);
  const [draft, setDraft] = useState("");
  const [busy, setBusy] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [history, setHistory] = useState<
    { id: number; body: string; savedAt: string }[] | null
  >(null);
  // The hash the draft was started from. Held in a ref, not state, so a
  // re-render between typing and saving cannot substitute a fresher one and
  // defeat the check.
  const base = useRef<string | null>(null);

  const load = useCallback(() => {
    api
      .brain(projectId)
      .then((b) => {
        setBrain(b);
        setDraft(b.body);
        base.current = b.hash;
      })
      .catch(() => {});
  }, [projectId]);
  useEffect(load, [load]);

  if (!brain) return null;

  const dirty = draft !== brain.body;
  const over = draft.length > brain.maxChars;

  const save = async (enabled = brain.enabled) => {
    setBusy(true);
    setError(null);
    setNote(null);
    try {
      const saved = await api.saveBrain(projectId, {
        body: draft,
        enabled,
        hash: base.current ?? undefined,
      });
      setBrain(saved);
      setDraft(saved.body);
      base.current = saved.hash;
      setNote("Saved. Every run in this project starts with it now.");
      setHistory(null);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold">Brain</h2>
          <p className="mt-0.5 max-w-xl text-xs text-ink-dim">
            What every run in this project should already know — where things live, how it
            is deployed, what not to touch. It reaches every card, every chat and every
            reply here, without being attached to anything.
          </p>
        </div>
        <label className="flex shrink-0 items-center gap-2 text-xs">
          <input
            type="checkbox"
            checked={brain.enabled}
            disabled={busy}
            onChange={(e) => save(e.target.checked)}
          />
          <span className={brain.enabled ? "text-ink" : "text-ink-dim"}>
            {brain.enabled ? "In use" : "Off"}
          </span>
        </label>
      </div>

      {!brain.enabled && (
        <p className="mb-3 max-w-2xl rounded-lg bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-900">
          Off, so runs behave as though this were empty. It is still here — turning it off
          is how you check whether it is the reason a run went wrong.
        </p>
      )}

      <textarea
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        disabled={busy}
        rows={16}
        placeholder={
          "This stack runs from compose.yaml; the frontend is on 9000.\n" +
          "The API lives in /backend. Tests are `pnpm test`.\n" +
          "Do not add dependencies without asking."
        }
        className="w-full max-w-2xl resize-y rounded-xl border border-line bg-surface p-3 font-mono text-xs leading-relaxed outline-none focus:border-accent disabled:opacity-60"
      />

      <div className="mt-2 flex max-w-2xl flex-wrap items-center gap-3">
        <button
          onClick={() => save()}
          disabled={busy || !dirty || over}
          className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-40"
        >
          {busy ? "Saving…" : dirty ? "Save" : "Saved"}
        </button>
        {/* Counted against the budget rather than silently truncated at the
            far end, where the loss would only show up as an agent that had not
            read the last paragraph. */}
        <span className={`text-[11px] ${over ? "font-medium text-danger" : "text-ink-dim"}`}>
          {draft.length.toLocaleString()} / {brain.maxChars.toLocaleString()} characters
          {over && " — too long to fit in a prompt"}
        </span>
        {brain.updatedAt && !dirty && (
          <span className="text-[11px] text-ink-dim">
            saved {new Date(brain.updatedAt).toLocaleString()}
          </span>
        )}
        <button
          onClick={() =>
            history
              ? setHistory(null)
              : api.brainRevisions(projectId).then((r) => setHistory(r.revisions)).catch(() => {})
          }
          className="ml-auto text-[11px] text-ink-dim underline hover:text-ink"
        >
          {history ? "hide history" : "history"}
        </button>
      </div>

      {error && (
        <div className="mt-3 max-w-2xl whitespace-pre-wrap rounded-lg bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-danger">
          {error}
        </div>
      )}
      {note && <div className="mt-3 text-[11px] text-ink-dim">{note}</div>}

      {history && (
        <div className="mt-4 max-w-2xl">
          <div className="text-xs font-medium">Earlier versions</div>
          {history.length === 0 ? (
            <p className="mt-1 text-[11px] text-ink-dim">
              Nothing yet — the previous text is kept from your next save onwards.
            </p>
          ) : (
            <div className="mt-2 flex flex-col gap-2">
              {history.map((r) => (
                <div key={r.id} className="rounded-lg border border-line bg-panel p-2.5">
                  <div className="flex items-baseline justify-between gap-2">
                    <span className="text-[11px] text-ink-dim">
                      {new Date(r.savedAt).toLocaleString()}
                    </span>
                    {/* Into the editor, not straight to the database: restoring
                        is a save like any other, so it is reviewed and it keeps
                        the version it replaced. */}
                    <button
                      onClick={() => setDraft(r.body)}
                      className="text-[11px] text-accent underline"
                    >
                      put this in the editor
                    </button>
                  </div>
                  <pre className="mt-1 max-h-24 overflow-y-auto whitespace-pre-wrap font-mono text-[11px] text-ink-dim">
                    {r.body}
                  </pre>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      <p className="mt-6 max-w-2xl text-[11px] leading-relaxed text-ink-dim">
        <span className="font-medium text-ink">No secrets here.</span> This text goes into
        a prompt and stays readable to anyone who opens this page, so a save containing
        something key-shaped is refused. Keep credentials in your shell or a password
        manager.
      </p>
    </div>
  );
}
