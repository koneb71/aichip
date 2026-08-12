import { useEffect, useState } from "react";
import { useOutletContext } from "react-router-dom";
import { Article, api } from "../../lib/api";
import { KnowledgeContext } from "./KnowledgeLayout";
import { Card, Empty, Item, Page, PageHead, SectionLabel, Stagger } from "../../components/ui/Surface";
import { Icon } from "../../components/ui/Icon";

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
    <Page>
      <PageHead
        title="Knowledge base"
        subtitle="Runbooks, conventions, architecture notes. Attach a page to a card and the agent working it reads the page before it starts."
      />

      {recent.length === 0 ? (
        <Empty
          icon={<Icon name="knowledge" size={28} />}
          title="Nothing here yet"
          hint="Make a page, or ask an agent to write one — it arrives as a proposal you accept, never as an overwrite."
        />
      ) : (
        <>
          <SectionLabel>Recently updated</SectionLabel>
          <Stagger className="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
            {recent.map((a) => (
              <Item key={a.id}>
                <Card to={`/knowledge/${a.id}`} className="h-full p-4">
                  <div className="flex items-start justify-between gap-2">
                    <span className="flex min-w-0 items-baseline gap-1.5 text-sm font-semibold">
                      <span className="shrink-0">{a.icon || "▦"}</span>
                      <span className="min-w-0">{a.title}</span>
                    </span>
                    {a.status === "draft" && (
                      <span className="shrink-0 rounded-md bg-amber-100 px-1.5 py-0.5 text-[10px] font-medium text-amber-800">
                        draft
                      </span>
                    )}
                  </div>
                  <div className="mt-2 line-clamp-3 text-xs leading-relaxed text-ink-dim">
                    {a.summary || "Empty"}
                  </div>
                  <div className="mt-3 flex items-center gap-1.5 text-[11px] text-ink-dim/80">
                    <Icon name="clock" size={12} />
                    {new Date(a.updatedAt).toLocaleDateString()}
                    {/* Worth saying on the card rather than only inside: a page
                        an agent wrote is one you have not read yet. */}
                    {a.origin === "agent" && (
                      <span className="ml-auto inline-flex items-center gap-1 rounded-full bg-tint-violet px-1.5 py-0.5 text-ink-violet">
                        <Icon name="sparkle" size={10} />
                        by an agent
                      </span>
                    )}
                  </div>
                </Card>
              </Item>
            ))}
          </Stagger>
        </>
      )}
    </Page>
  );
}
