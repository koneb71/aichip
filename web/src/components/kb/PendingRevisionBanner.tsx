import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { useNavigate } from "react-router-dom";
import { api, Revision, RevisionDiff } from "../../lib/api";
import { annotateDiff } from "../../lib/diff";
import { Button } from "../ui/Button";
import { Textarea } from "../ui/Field";

/**
 * An agent has rewritten this page and is waiting for you.
 *
 * This is the whole reason the revision log exists. Before it, a documentation
 * run replaced whatever was there — no copy, no diff, no trace — so a page you
 * had carefully corrected could be silently reverted by asking for a typo fix.
 * Now the page is untouched until someone says otherwise.
 *
 * The diff is over the *text* projection, not the HTML: two model passes over
 * identical prose emit different markup, and a diff that reports every line
 * changed is a diff nobody reads, which turns review into rubber-stamping.
 */
export function PendingRevisionBanner({
  pageId,
  revision,
  currentSeq,
  onDecided,
}: {
  pageId: string;
  revision: Revision;
  currentSeq: number;
  onDecided: () => void;
}) {
  const navigate = useNavigate();
  const [diff, setDiff] = useState<RevisionDiff | null>(null);
  const [expanded, setExpanded] = useState(false);
  const [asking, setAsking] = useState(false);
  const [note, setNote] = useState("");
  const [busy, setBusy] = useState<"accept" | "edit" | "discard" | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    api
      .revisionDiff(pageId, revision.seq)
      .then((d) => live && setDiff(d))
      .catch(() => {});
    return () => {
      live = false;
    };
  }, [pageId, revision.seq]);

  // The page moved on while the agent was working. Saying so is the whole
  // difference between a review and a silent merge.
  const stale = revision.baseSeq !== null && revision.baseSeq < currentSeq;

  const act = async (kind: "accept" | "edit" | "discard", fn: () => Promise<unknown>) => {
    setBusy(kind);
    setError(null);
    try {
      await fn();
      if (kind === "edit") navigate(`/knowledge/${pageId}/edit`);
      else onDecided();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(null);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0, y: -4 }}
      animate={{ opacity: 1, y: 0 }}
      className="mb-5 rounded-xl border border-warning/40 bg-warning-subtle p-4"
    >
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <span className="text-sm font-semibold text-warning-fg">
          An agent proposed a revision
          {diff && (
            <span className="ml-1.5 font-normal">
              · +{diff.added} −{diff.removed} lines
            </span>
          )}
        </span>
        <Button variant="ghost" size="xs" onClick={() => setExpanded((v) => !v)}>
          {expanded ? "Hide the diff" : "Show the diff"}
        </Button>
      </div>
      <p className="mt-1 text-xs text-fg">
        Nothing has changed yet — this page is exactly as you left it.
      </p>
      {stale && (
        <p className="mt-1.5 text-xs font-medium text-warning-fg">
          You edited this page after the agent started. The diff below is
          against revision {revision.baseSeq}, not the version you are reading.
        </p>
      )}

      {expanded && diff && (
        <div className="mt-3 max-h-96 overflow-auto rounded-lg border border-border bg-panel">
          <DiffBody unified={diff.diff} />
        </div>
      )}

      {error && (
        <div className="mt-2 rounded-lg bg-danger-subtle px-3 py-1.5 text-xs text-danger-fg">
          {error}
        </div>
      )}

      {asking ? (
        <div className="mt-3">
          <Textarea
            autoFocus
            value={note}
            onChange={(e) => setNote(e.target.value)}
            rows={2}
            placeholder="What was wrong with it? Kept on the page's history."
            className="resize-none"
          />
          <div className="mt-2 flex gap-2">
            <Button
              size="sm"
              disabled={!!busy}
              onClick={() =>
                act("discard", () =>
                  api.discardRevision(pageId, revision.seq, note.trim()),
                )
              }
            >
              {busy === "discard" ? "Discarding…" : "Discard it"}
            </Button>
            <Button variant="ghost" size="sm" onClick={() => setAsking(false)}>
              Cancel
            </Button>
          </div>
        </div>
      ) : (
        <div className="mt-3 flex flex-wrap gap-2">
          <Button
            variant="primary"
            size="sm"
            disabled={!!busy}
            onClick={() => act("accept", () => api.acceptRevision(pageId, revision.seq))}
          >
            {busy === "accept" ? "Accepting…" : "Accept"}
          </Button>
          {/* The 90%-right case. Retyping feedback and paying for another pass
              to fix one line is worse than fixing the line. */}
          <Button
            size="sm"
            disabled={!!busy}
            onClick={() => act("edit", () => api.acceptRevision(pageId, revision.seq))}
          >
            Accept and edit
          </Button>
          <Button size="sm" disabled={!!busy} onClick={() => setAsking(true)}>
            Discard
          </Button>
        </div>
      )}
    </motion.div>
  );
}

/** Render a unified diff through the same pipeline code review already uses. */
export function DiffBody({ unified }: { unified: string }) {
  const lines = annotateDiff(unified);
  return (
    <pre className="overflow-x-auto p-2 font-mono text-[11px] leading-relaxed">
      {lines.map((l, i) => (
        <div
          key={i}
          className={
            l.kind === "add"
              ? "bg-success-subtle text-success-fg"
              : l.kind === "del"
                ? "bg-danger-subtle text-danger-fg"
                : l.kind === "meta" || l.kind === "hunk"
                  ? "text-fg-muted"
                  : ""
          }
        >
          {l.text || " "}
        </div>
      ))}
    </pre>
  );
}
