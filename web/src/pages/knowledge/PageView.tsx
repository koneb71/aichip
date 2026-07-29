import { useCallback, useEffect, useState } from "react";
import { Link, useNavigate, useOutletContext, useParams } from "react-router-dom";
import { motion } from "framer-motion";
import { Article, api } from "../../lib/api";
import { PageBody } from "../../components/kb/PageBody";
import { PageToc } from "../../components/kb/PageToc";
import { PendingRevisionBanner } from "../../components/kb/PendingRevisionBanner";
import { MovePicker } from "../../components/kb/MovePicker";
import { IconPicker } from "../../components/kb/IconPicker";
import { GenerateModal } from "../../components/kb/GenerateModal";
import { KnowledgeContext } from "./KnowledgeLayout";

/**
 * A page, at its own address.
 *
 * Reading is separate from editing on purpose. An always-editable document puts
 * every reader one keystroke from changing it, which is the accidental
 * overwrite the revision log exists to prevent — and it would drag the editor
 * bundle, larger than the whole rest of the app, into every read.
 */
export default function PageView() {
  const { pageId } = useParams();
  const navigate = useNavigate();
  const { reload, workspaceId, pages } = useOutletContext<KnowledgeContext>();
  const [page, setPage] = useState<Article | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [moving, setMoving] = useState(false);
  const [revising, setRevising] = useState(false);

  const load = useCallback(async () => {
    if (!pageId) return;
    try {
      setPage(await api.article(pageId));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  }, [pageId]);

  useEffect(() => {
    setPage(null);
    setError(null);
    load();
  }, [load]);

  // A page an agent is still writing fills in on its own; polling stops the
  // moment it lands rather than running forever.
  useEffect(() => {
    if (!page?.writing) return;
    const t = setInterval(() => {
      load();
      reload();
    }, 3000);
    return () => clearInterval(t);
  }, [page?.writing, load, reload]);

  if (error) {
    return <div className="p-8 text-sm text-danger">{error}</div>;
  }
  if (!page) {
    return <div className="p-8 text-sm text-ink-dim">Loading…</div>;
  }

  const empty = !page.contentHtml?.trim();

  return (
    <div className="h-full overflow-y-auto">
      <div className="mx-auto flex max-w-5xl gap-8 px-6 py-6 lg:px-10">
        <div className="min-w-0 flex-1">
          <nav className="flex flex-wrap items-center gap-1 text-xs text-ink-dim">
            {page.breadcrumb?.map((c, i) => (
              <span key={c.id} className="flex items-center gap-1">
                {i > 0 && <span className="opacity-50">/</span>}
                {c.id === page.id ? (
                  <span className="text-ink">{c.title}</span>
                ) : (
                  <Link to={`/knowledge/${c.id}`} className="hover:text-accent">
                    {c.icon} {c.title}
                  </Link>
                )}
              </span>
            ))}
          </nav>

          {page.pendingRevision && (
            <div className="mt-4">
              <PendingRevisionBanner
                pageId={page.id}
                revision={page.pendingRevision}
                currentSeq={page.currentSeq}
                onDecided={() => {
                  load();
                  reload();
                }}
              />
            </div>
          )}

          <div className="mt-4 flex items-start gap-3">
            <IconPicker
              value={page.icon}
              onChange={async (icon) => {
                await api.updateArticle(page.id, { icon });
                load();
                reload();
              }}
            />
            <h1 className="min-w-0 flex-1 text-3xl font-bold tracking-tight">
              {page.title}
            </h1>
          </div>

          <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-xs text-ink-dim">
            <span>{page.status === "published" ? "Published" : "Draft"}</span>
            <span>·</span>
            <span>updated {new Date(page.updatedAt).toLocaleDateString()}</span>
            {page.currentSeq > 0 && (
              <>
                <span>·</span>
                <Link to={`/knowledge/${page.id}/history`} className="hover:text-accent">
                  revision {page.currentSeq}
                </Link>
              </>
            )}
            {page.origin === "agent" && (
              <>
                <span>·</span>
                <span>written by an agent, accepted by you</span>
              </>
            )}
          </div>

          <div className="mt-4 flex flex-wrap gap-2">
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={() => navigate(`/knowledge/${page.id}/edit`)}
              className="rounded-lg bg-accent px-3.5 py-1.5 text-xs font-medium text-white"
            >
              Edit
            </motion.button>
            <button
              onClick={() => setRevising(true)}
              className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:bg-panel-2"
            >
              Ask an agent to revise
            </button>
            <Link
              to={`/knowledge/${page.id}/history`}
              className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:bg-panel-2"
            >
              History
            </Link>
            <button
              onClick={() => setMoving(true)}
              className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:bg-panel-2"
            >
              Move
            </button>
            <button
              onClick={async () => {
                await api.updateArticle(page.id, {
                  status: page.status === "published" ? "draft" : "published",
                });
                load();
                reload();
              }}
              className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:bg-panel-2"
            >
              {page.status === "published" ? "Unpublish" : "Publish"}
            </button>
          </div>

          {/* A revision in flight is a banner, not a blindfold. Hiding the
              body meant asking an agent to fix a typo took the page offline
              for everyone until the run finished. */}
          {page.writing && !empty && (
            <div className="mt-4 rounded-lg border border-line bg-panel-2 px-3 py-2 text-xs text-ink-dim">
              An agent is drafting a revision. This page is unchanged until you
              accept it.
            </div>
          )}

          <div className="mt-6">
            {page.writing && empty ? (
              <div className="rounded-xl border border-dashed border-line px-4 py-10 text-center text-sm text-ink-dim">
                An agent is writing this page…
              </div>
            ) : empty ? (
              <div className="rounded-xl border border-dashed border-line px-4 py-10 text-center text-sm text-ink-dim">
                This page is empty.{" "}
                <button
                  onClick={() => navigate(`/knowledge/${page.id}/edit`)}
                  className="text-accent hover:underline"
                >
                  Write something
                </button>
                .
              </div>
            ) : (
              <PageBody html={page.contentHtml!} />
            )}
          </div>

          {!!page.children?.length && (
            <section className="mt-10 border-t border-line pt-5">
              <h2 className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
                Inside this page
              </h2>
              <div className="mt-2 grid gap-2 sm:grid-cols-2">
                {page.children.map((c) => (
                  <Link
                    key={c.id}
                    to={`/knowledge/${c.id}`}
                    className="rounded-xl border border-line bg-panel p-3 hover:bg-panel-2"
                  >
                    <div className="text-sm font-medium">
                      {c.icon || "▦"} {c.title}
                    </div>
                    <div className="mt-0.5 line-clamp-2 text-xs text-ink-dim">
                      {c.summary}
                    </div>
                  </Link>
                ))}
              </div>
            </section>
          )}

          {!!page.backlinks?.length && (
            <section className="mt-8 border-t border-line pt-5">
              <h2 className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
                Linked from
              </h2>
              <div className="mt-2 flex flex-wrap gap-1.5">
                {page.backlinks.map((b) => (
                  <Link
                    key={b.id}
                    to={`/knowledge/${b.id}`}
                    className="rounded-lg border border-line px-2.5 py-1 text-xs hover:bg-panel-2"
                  >
                    {b.icon || "▦"} {b.title}
                  </Link>
                ))}
              </div>
            </section>
          )}
        </div>

        <aside className="hidden w-48 shrink-0 lg:block">
          {page.contentHtml && <PageToc html={page.contentHtml} />}
        </aside>
      </div>

      {moving && (
        <MovePicker
          pages={pages}
          pageId={page.id}
          onClose={() => setMoving(false)}
          onMoved={() => {
            setMoving(false);
            load();
            reload();
          }}
        />
      )}
      {revising && (
        <GenerateModal
          workspaceId={workspaceId}
          articleId={page.id}
          defaultProjectId={page.projectId}
          onClose={() => setRevising(false)}
          onStarted={() => {
            setRevising(false);
            load();
            reload();
          }}
        />
      )}
    </div>
  );
}
