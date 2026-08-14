import { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import { Article, api } from "../../lib/api";

/**
 * Pick knowledge-base articles to hand an agent.
 *
 * Not decoration: a tagged article is folded into the run's prompt, so this is
 * how you say "read the runbook before you touch anything". The wording in the
 * UI says so, because a picker that looks like tagging invites people to tag
 * everything, and a prompt with six articles in it is worse than one with the
 * right one.
 */
export function ArticlePicker({
  workspaceId,
  selected,
  onChange,
  compact = false,
}: {
  workspaceId: string;
  selected: string[];
  /** The second argument is what was chosen, not just its ids — a caller that
   *  has to *show* the choice back (a chat message, say) would otherwise have
   *  to fetch the same list again to learn a title it already had on screen.
   *  Callers that only need ids can ignore it. */
  onChange: (ids: string[], chosen: Article[]) => void;
  /** Inline in a composer rather than as a labelled block. */
  compact?: boolean;
}) {
  const [articles, setArticles] = useState<Article[]>([]);
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");

  useEffect(() => {
    api
      .articles(workspaceId)
      .then((r) => setArticles(r.articles))
      .catch(() => {});
  }, [workspaceId]);

  const chosen = useMemo(
    () => articles.filter((a) => selected.includes(a.id)),
    [articles, selected],
  );
  // Server-side, so a page is findable by something in its body rather than
  // only by its title — which is how people actually remember pages.
  useEffect(() => {
    const q = query.trim();
    if (!q) return;
    let stale = false;
    const t = setTimeout(() => {
      api
        .articles(workspaceId, q)
        .then((r) => !stale && setArticles(r.articles))
        .catch(() => {});
    }, 180);
    return () => {
      stale = true;
      clearTimeout(t);
    };
  }, [query, workspaceId]);

  const matches = useMemo(
    () => articles.filter((a) => !selected.includes(a.id)).slice(0, 8),
    [articles, selected],
  );

  // Nothing to pick from is not an empty picker, it's no picker — an control
  // that can never do anything is just clutter on every card.
  if (articles.length === 0) return null;

  const toggle = (id: string) => {
    const next = selected.includes(id) ? selected.filter((x) => x !== id) : [...selected, id];
    // Ordered by the selection, not by the article list: the order somebody
    // attached things in is a statement about what matters most, and the
    // prompt preserves it.
    onChange(
      next,
      next.map((x) => articles.find((a) => a.id === x)).filter((a): a is Article => !!a),
    );
  };

  return (
    <div>
      {!compact && (
        <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          Read first
        </div>
      )}
      <div className="flex flex-wrap items-center gap-1.5">
        {chosen.map((a) => (
          <motion.span
            key={a.id}
            layout
            initial={{ opacity: 0, scale: 0.95 }}
            animate={{ opacity: 1, scale: 1 }}
            className="flex items-center gap-1 rounded-lg border border-accent/40 bg-accent/5 px-2 py-1 text-xs"
          >
            <span className="max-w-48 truncate">{a.title}</span>
            <button
              onClick={() => toggle(a.id)}
              className="text-ink-dim hover:text-danger"
              title="Remove"
            >
              ✕
            </button>
          </motion.span>
        ))}
        <button
          onClick={() => setOpen((v) => !v)}
          className="rounded-lg border border-dashed border-line px-2 py-1 text-xs text-ink-dim hover:border-accent hover:text-accent"
        >
          {chosen.length ? "+ article" : "+ knowledge-base article"}
        </button>
      </div>

      {open && (
        <motion.div
          initial={{ opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          className="mt-2 rounded-xl border border-line bg-panel p-2"
        >
          <input
            autoFocus
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder="Search the knowledge base…"
            className="mb-1.5 w-full rounded-lg border border-line bg-panel px-2.5 py-1.5 text-xs outline-none focus:border-accent"
          />
          {matches.length === 0 ? (
            <div className="px-2 py-3 text-center text-xs text-ink-dim">
              {query ? "Nothing matches." : "Everything is already attached."}
            </div>
          ) : (
            <div className="max-h-56 overflow-y-auto">
              {matches.map((a) => (
                <button
                  key={a.id}
                  onClick={() => {
                    toggle(a.id);
                    setQuery("");
                  }}
                  className="block w-full rounded-lg px-2 py-1.5 text-left hover:bg-panel-2"
                >
                  <span className="flex items-center gap-1.5">
                    <span className="truncate text-xs font-medium">
                      {a.icon || "▦"} {a.title}
                    </span>
                    {a.status === "draft" && (
                      <span className="shrink-0 rounded bg-amber-100 px-1 text-[9px] text-amber-800">
                        draft
                      </span>
                    )}
                  </span>
                  <span className="line-clamp-1 text-[11px] text-ink-dim">{a.summary}</span>
                </button>
              ))}
            </div>
          )}
          <p className="mt-1 px-2 text-[10px] text-ink-dim">
            Attached articles are put in front of the agent before it starts.
            Attach the one that matters, not everything.
          </p>
        </motion.div>
      )}
    </div>
  );
}
