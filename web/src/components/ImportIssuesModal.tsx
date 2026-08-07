import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, type GitHubIssue } from "../lib/api";

/**
 * Turn GitHub issues into board cards.
 *
 * Nothing is ticked to begin with and there is no "import all", and that is
 * the security control rather than a UI preference. An issue body becomes an
 * agent's prompt, and on a public repository anyone on the internet wrote it —
 * so the person choosing sees the text first, one issue at a time.
 *
 * Bodies render as **plain text**, never through the markdown component. An
 * issue must not be able to put a tracking pixel or a dressed-up link into the
 * dashboard, which is a separate exposure from the prompt itself.
 */
export function ImportIssuesModal({
  projectId,
  onClose,
  onImported,
}: {
  projectId: string;
  onClose: () => void;
  onImported: () => void;
}) {
  const [repo, setRepo] = useState<string | null>(null);
  const [issues, setIssues] = useState<GitHubIssue[]>([]);
  const [publicRepo, setPublicRepo] = useState(true);
  const [refusal, setRefusal] = useState<string | null>(null);
  const [chosen, setChosen] = useState<Set<number>>(new Set());
  const [open, setOpen] = useState<number | null>(null);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .githubIssues(projectId)
      .then((r) => {
        setRepo(r.repo);
        setIssues(r.issues);
        setPublicRepo(r.public ?? true);
        setRefusal(r.refusal);
      })
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")))
      .finally(() => setLoading(false));
  }, [projectId]);

  const toggle = (n: number) =>
    setChosen((prev) => {
      const next = new Set(prev);
      if (next.has(n)) next.delete(n);
      else next.add(n);
      return next;
    });

  const importChosen = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.importIssues(projectId, [...chosen]);
      onImported();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  const importable = issues.filter((i) => !i.importedAs);

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.97, y: 8 }}
        transition={{ type: "spring", stiffness: 420, damping: 32 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex max-h-[88vh] w-full max-w-3xl flex-col rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">
          Import issues{repo && <span className="ml-2 font-mono text-xs text-ink-dim">{repo}</span>}
        </h3>

        {/* Said before anything is ticked, because it changes what "read this
            first" means. */}
        {publicRepo && !refusal && (
          <p className="mt-2 rounded-lg bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-900">
            Anyone on the internet can open an issue on a public repository, and an
            imported issue becomes an agent&rsquo;s instructions. Read each one before you
            tick it. Imported cards always land in Backlog and never start on their own.
          </p>
        )}

        {loading && <p className="mt-4 text-xs text-ink-dim">Asking GitHub…</p>}
        {refusal && <p className="mt-4 text-xs text-ink-dim">{refusal}</p>}
        {!loading && !refusal && issues.length === 0 && (
          <p className="mt-4 text-xs text-ink-dim">No open issues.</p>
        )}

        <div className="mt-3 min-h-0 flex-1 space-y-1 overflow-y-auto">
          {issues.map((issue) => {
            const done = Boolean(issue.importedAs);
            return (
              <div
                key={issue.number}
                className={`rounded-lg border px-3 py-2 ${
                  done ? "border-line bg-panel-2 opacity-60" : "border-line"
                }`}
              >
                <div className="flex items-start gap-2">
                  <input
                    type="checkbox"
                    className="mt-1"
                    checked={chosen.has(issue.number)}
                    disabled={done || busy}
                    onChange={() => toggle(issue.number)}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-baseline gap-x-2 text-sm">
                      <span className="font-mono text-xs text-ink-dim">#{issue.number}</span>
                      {/* Plain text. React escapes it; nothing renders it as markup. */}
                      <span className="min-w-0 break-words font-medium">{issue.title}</span>
                      {issue.author && (
                        <span className="text-[11px] text-ink-dim">by @{issue.author}</span>
                      )}
                      {done && <span className="text-[11px] text-ink-dim">· already a card</span>}
                    </div>
                    {issue.labels.length > 0 && (
                      <div className="mt-1 flex flex-wrap gap-1">
                        {issue.labels.map((l) => (
                          <span
                            key={l}
                            className="rounded-full bg-panel-2 px-2 py-0.5 text-[10px] text-ink-dim"
                          >
                            {l}
                          </span>
                        ))}
                      </div>
                    )}
                    <button
                      onClick={() => setOpen(open === issue.number ? null : issue.number)}
                      className="mt-1 text-[11px] text-accent hover:underline"
                    >
                      {open === issue.number ? "Hide" : "Read"} what it says
                    </button>
                    {open === issue.number && (
                      // Monospace, plain, scroll-capped: this is the text that
                      // becomes a prompt, shown as text.
                      <pre className="mt-1 max-h-56 overflow-auto whitespace-pre-wrap rounded-lg bg-surface p-2 font-mono text-[11px] leading-relaxed text-ink-dim">
                        {issue.body || "(no description)"}
                      </pre>
                    )}
                  </div>
                  <a
                    href={issue.url}
                    target="_blank"
                    rel="noreferrer"
                    className="shrink-0 text-[11px] text-ink-dim hover:text-ink"
                  >
                    open ↗
                  </a>
                </div>
              </div>
            );
          })}
        </div>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-[11px] text-danger">{error}</div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={importChosen}
            disabled={busy || chosen.size === 0}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy
              ? "Importing…"
              : chosen.size === 0
                ? "Choose issues to import"
                : `Import ${chosen.size} as ${chosen.size === 1 ? "a card" : "cards"}`}
          </motion.button>
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Cancel
          </button>
          {importable.length > 0 && (
            <span className="ml-auto text-[11px] text-ink-dim">
              {importable.length} not yet imported
            </span>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
