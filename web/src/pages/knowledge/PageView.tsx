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
import { Icon } from "../../components/ui/Icon";

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
                {i > 0 && (
                  <span className="opacity-40">
                    <Icon name="chevronRight" size={11} />
                  </span>
                )}
                {c.id === page.id ? (
                  <span className="text-ink">{c.title}</span>
                ) : (
                  <Link
                    to={`/knowledge/${c.id}`}
                    className="rounded transition-colors hover:text-accent"
                  >
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
              className="ring-focus inline-flex items-center gap-1.5 rounded-xl bg-accent px-3.5 py-2 text-xs font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
            >
              Edit
            </motion.button>
            <button
              onClick={() => setRevising(true)}
              className="ring-focus inline-flex items-center gap-1.5 rounded-xl border border-line px-3.5 py-2 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
            >
              Ask an agent to revise
            </button>
            <Link
              to={`/knowledge/${page.id}/history`}
              className="ring-focus inline-flex items-center gap-1.5 rounded-xl border border-line px-3.5 py-2 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
            >
              History
            </Link>
            <button
              onClick={() => setMoving(true)}
              className="ring-focus inline-flex items-center gap-1.5 rounded-xl border border-line px-3.5 py-2 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
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
              className="ring-focus inline-flex items-center gap-1.5 rounded-xl border border-line px-3.5 py-2 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
            >
              {page.status === "published" ? "Unpublish" : "Publish"}
            </button>
          </div>

          {/* A revision in flight is a banner, not a blindfold. Hiding the
              body meant asking an agent to fix a typo took the page offline
              for everyone until the run finished. */}
          {page.writing && !empty && (
            <div className="mt-4 rounded-xl border border-line bg-panel-2 px-3.5 py-2.5 text-xs text-ink-dim">
              An agent is drafting a revision. This page is unchanged until you
              accept it.
            </div>
          )}

          <div className="mt-6 max-w-[68ch]">
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
              <div className="mt-3 grid gap-3 sm:grid-cols-2">
                {page.children.map((c) => (
                  <Link
                    key={c.id}
                    to={`/knowledge/${c.id}`}
                    className="lift ring-focus card-shadow rounded-2xl border border-line bg-panel p-3.5 hover:border-ink-dim/25"
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

          {!!page.usedBy?.tasks.length && (
            <section className="mt-8 border-t border-line pt-5">
              <h2 className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
                Used by
              </h2>
              <p className="mt-1 text-xs text-ink-dim">
                Cards that hand this page to an agent. Editing it changes what
                they are working from.
              </p>
              <div className="mt-2 space-y-1.5">
                {page.usedBy.tasks.map((t) => (
                  <Link
                    key={t.id}
                    // Deep link, not just the board: landing on a column of
                    // forty cards and asking someone to find the right one is
                    // not a link, it is a hint.
                    to={`/projects/${t.projectId}?task=${t.id}`}
                    className="flex items-center gap-2 rounded-xl border border-line bg-panel px-3 py-2 hover:bg-panel-2"
                  >
                    <span className="min-w-0 flex-1 truncate text-sm">{t.title}</span>
                    <span className="shrink-0 text-[11px] text-ink-dim">
                      {t.projectName} · {t.boardColumn}
                    </span>
                    <span
                      className={`shrink-0 rounded-md px-1.5 py-0.5 text-[10px] font-medium ${
                        t.attached
                          ? "bg-accent/10 text-accent"
                          : "bg-panel-2 text-ink-dim"
                      }`}
                      title={
                        t.attached
                          ? "Attached to the card, so every run on it is given this page"
                          : "Linked from a comment, so it reached one reply"
                      }
                    >
                      {t.attached
                        ? "attached"
                        : `${t.mentions} ${t.mentions === 1 ? "mention" : "mentions"}`}
                    </span>
                  </Link>
                ))}
              </div>
              {page.usedBy.total > page.usedBy.tasks.length && (
                <p className="mt-2 text-xs text-ink-dim">
                  and {page.usedBy.total - page.usedBy.tasks.length} more
                </p>
              )}
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
                    className="ring-focus inline-flex items-center gap-1.5 rounded-xl border border-line px-2.5 py-1.5 text-xs transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
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
