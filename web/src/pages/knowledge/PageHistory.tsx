import { useCallback, useEffect, useState } from "react";
import { Link, useOutletContext, useParams } from "react-router-dom";
import { motion } from "framer-motion";
import { api, Revision, RevisionDiff } from "../../lib/api";
import { DiffBody } from "../../components/kb/PendingRevisionBanner";
import { KnowledgeContext } from "./KnowledgeLayout";

/**
 * What this page has been.
 *
 * The trail is complete on purpose: discarded proposals and superseded ones
 * stay listed. A history with holes in it is not a history — the interesting
 * question is often "what did we decide not to do", and deleting the rejected
 * version deletes the answer.
 */
export default function PageHistory() {
  const { pageId } = useParams();
  const { reload } = useOutletContext<KnowledgeContext>();
  const [revisions, setRevisions] = useState<Revision[]>([]);
  const [selected, setSelected] = useState<number | null>(null);
  const [diff, setDiff] = useState<RevisionDiff | null>(null);
  const [confirming, setConfirming] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);

  const load = useCallback(async () => {
    if (!pageId) return;
    setRevisions((await api.revisions(pageId)).revisions);
  }, [pageId]);

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    // Cleared on every change, not only when nothing is selected: switching
    // straight from one revision to another left the previous diff on screen
    // under the new heading, which is worse than showing nothing.
    setDiff(null);
    if (!pageId || selected === null) return;
    let stale = false;
    api
      .revisionDiff(pageId, selected)
      .then((d) => !stale && setDiff(d))
      .catch(() => {});
    return () => {
      stale = true;
    };
  }, [pageId, selected]);

  if (!pageId) return null;

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto max-w-4xl px-6 py-6 lg:px-10">
        <Link to={`/knowledge/${pageId}`} className="text-xs text-ink-dim hover:text-ink">
          ← Back to the page
        </Link>
        <h1 className="mt-3 text-2xl font-bold tracking-tight">History</h1>
        <p className="mt-1 text-sm text-ink-dim">
          Every version this page has had, including the ones that were turned
          down. Restoring writes a new revision rather than rewinding — history
          is a record of what happened.
        </p>

        <div className="mt-5 space-y-1.5">
          {revisions.map((r) => (
            <motion.div
              key={r.seq}
              layout
              className={`rounded-xl border bg-panel p-3 ${
                selected === r.seq ? "border-accent" : "border-line"
              }`}
            >
              <div className="flex flex-wrap items-center gap-2">
                <button
                  onClick={() => setSelected(selected === r.seq ? null : r.seq)}
                  className="min-w-0 flex-1 text-left"
                >
                  <span className="text-sm font-medium">
                    {r.authorKind === "agent" ? "◆" : "●"} Revision {r.seq}
                  </span>
                  <span className="ml-2 text-xs text-ink-dim">{r.title}</span>
                </button>
                <StateBadge revision={r} />
                <span className="text-[11px] text-ink-dim">
                  {new Date(r.createdAt).toLocaleString()}
                </span>
              </div>

              {r.note && (
                <div className="mt-1 text-xs italic text-ink-dim">“{r.note}”</div>
              )}

              {selected === r.seq && (
                <div className="mt-3">
                  {diff && (
                    <>
                      <div className="mb-1.5 text-[11px] text-ink-dim">
                        {diff.from === null
                          ? "against an empty page"
                          : `against revision ${diff.from}`}{" "}
                        · +{diff.added} −{diff.removed}
                      </div>
                      <div className="max-h-80 overflow-auto rounded-lg border border-line">
                        <DiffBody unified={diff.diff} />
                      </div>
                    </>
                  )}
                  {r.state === "accepted" || r.state === "superseded" ? (
                    confirming === r.seq ? (
                      <div className="mt-2 flex items-center gap-2 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs">
                        <span className="text-amber-900">
                          Put revision {r.seq} back as the current version?
                        </span>
                        <button
                          disabled={busy}
                          onClick={async () => {
                            setBusy(true);
                            await api.restoreRevision(pageId, r.seq);
                            setBusy(false);
                            setConfirming(null);
                            load();
                            reload();
                          }}
                          className="rounded-lg bg-accent px-2.5 py-1 font-medium text-white"
                        >
                          Restore
                        </button>
                        <button
                          onClick={() => setConfirming(null)}
                          className="text-ink-dim hover:text-ink"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <button
                        onClick={() => setConfirming(r.seq)}
                        className="mt-2 rounded-lg border border-line px-3 py-1.5 text-xs hover:bg-panel-2"
                      >
                        Restore this version
                      </button>
                    )
                  ) : null}
                </div>
              )}
            </motion.div>
          ))}
        </div>
      </div>
    </div>
  );
}

function StateBadge({ revision }: { revision: Revision }) {
  const label =
    revision.state === "pending"
      ? "waiting for you"
      : revision.state === "discarded"
        ? "discarded"
        : revision.state === "superseded"
          ? "superseded"
          : revision.kind === "restore"
            ? `restored from ${revision.restoredFrom}`
            : revision.kind === "import"
              ? "imported"
              : null;
  if (!label) return null;
  return (
    <span
      className={`rounded-md px-1.5 py-0.5 text-[10px] font-medium ${
        revision.state === "pending"
          ? "bg-amber-100 text-amber-800"
          : "bg-panel-2 text-ink-dim"
      }`}
    >
      {label}
    </span>
  );
}
