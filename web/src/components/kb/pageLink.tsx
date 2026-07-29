import { MutableRefObject } from "react";
import Mention from "@tiptap/extension-mention";
import { api } from "../../lib/api";
import { menuRenderer } from "./suggestionMenu";

/**
 * `@` links one page to another.
 *
 * It is the only way to create a page link, and links are what populate the
 * backlink graph — the thing that turns a pile of documents into a wiki. So
 * this is not a nicety; without it the "linked from" panel is always empty.
 *
 * Rendered as a plain `<a href="/knowledge/:id">` rather than the extension's
 * default `<span data-type="mention">`, and that matters: the sanitiser allows
 * no arbitrary `data-*`, so the default markup would be stripped to bare text
 * on the first save and every link would quietly stop being one.
 */
export function pageLinkMention(workspaceId: MutableRefObject<string>) {
  return Mention.configure({
    HTMLAttributes: { class: "kb-page-link" },
    renderHTML({ node }) {
      return [
        "a",
        { href: `/knowledge/${node.attrs.id}`, class: "kb-page-link" },
        `${node.attrs.label ?? node.attrs.id}`,
      ];
    },
    renderText({ node }) {
      return node.attrs.label ?? "";
    },
    suggestion: {
      char: "@",
      items: async ({ query }: { query: string }) => {
        if (!query) return [];
        try {
          const r = await api.articles(workspaceId.current, query);
          return r.articles.slice(0, 8).map((a) => ({
            id: a.id,
            label: `${a.icon || "▦"} ${a.title}`.trim(),
            hint: a.status === "draft" ? "draft" : undefined,
          }));
        } catch {
          // A failed search should close the menu, not break typing.
          return [];
        }
      },
      render: menuRenderer,
    },
  });
}
