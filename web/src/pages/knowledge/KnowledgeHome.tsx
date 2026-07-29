import { useEffect, useState } from "react";
import { Link, useOutletContext } from "react-router-dom";
import { Article, api } from "../../lib/api";
import { KnowledgeContext } from "./KnowledgeLayout";

/**
 * What you see before you pick a page.
 *
 * Recently-updated rather than a full grid: the tree beside it is the index,
 * so repeating it here would be two lists of the same thing. What the index
 * can't tell you is what moved lately.
 */
export default function KnowledgeHome() {
  const { workspaceId } = useOutletContext<KnowledgeContext>();
  const [recent, setRecent] = useState<Article[]>([]);

  useEffect(() => {
    api
      .articles(workspaceId)
      .then((r) => setRecent(r.articles.slice(0, 12)))
      .catch(() => {});
  }, [workspaceId]);

  return (
    <div className="h-full overflow-y-auto px-6 py-8 lg:px-10">
      <h1 className="text-2xl font-bold tracking-tight">Knowledge base</h1>
      <p className="mt-1 max-w-xl text-sm text-ink-dim">
        Runbooks, conventions, architecture notes. Attach a page to a card and
        the agent working it reads the page before it starts.
      </p>

      {recent.length === 0 ? (
        <div className="mt-6 rounded-xl border border-dashed border-line px-4 py-12 text-center text-sm text-ink-dim">
          Nothing here yet. Make a page, or ask an agent to write one.
        </div>
      ) : (
        <>
          <h2 className="mt-8 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Recently updated
          </h2>
          <div className="mt-2 grid gap-2 sm:grid-cols-2 xl:grid-cols-3">
            {recent.map((a) => (
              <Link
                key={a.id}
                to={`/knowledge/${a.id}`}
                className="card-shadow rounded-xl border border-line bg-panel p-4 hover:bg-panel-2"
              >
                <div className="flex items-start justify-between gap-2">
                  <span className="text-sm font-semibold">
                    {a.icon || "▦"} {a.title}
                  </span>
                  {a.status === "draft" && (
                    <span className="shrink-0 rounded-md bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-800">
                      draft
                    </span>
                  )}
                </div>
                <div className="mt-1.5 line-clamp-3 text-xs text-ink-dim">
                  {a.summary || "Empty"}
                </div>
                <div className="mt-2 text-[11px] text-ink-dim">
                  {new Date(a.updatedAt).toLocaleDateString()}
                  {a.origin === "agent" && " · written by an agent"}
                </div>
              </Link>
            ))}
          </div>
        </>
      )}
    </div>
  );
}
