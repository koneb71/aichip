import { useEffect, useRef } from "react";
import { useNavigate } from "react-router-dom";

/**
 * A page's body, rendered.
 *
 * This is the only `dangerouslySetInnerHTML` in the app, and it is safe for one
 * specific reason: bodies are sanitised **on write**, never on read. The string
 * in the database has already been through `kb::sanitize` — scripts, event
 * handlers, `javascript:` and `data:` URLs and off-allowlist iframes are gone
 * before it is ever stored. Rendering is not the place that decision gets made.
 *
 * Links to other pages are intercepted so the wiki navigates like one document
 * rather than reloading the whole app.
 */
export function PageBody({ html }: { html: string }) {
  const ref = useRef<HTMLDivElement>(null);
  const navigate = useNavigate();

  // Colour code blocks after they are in the DOM.
  //
  // The editor highlights as you type, but `getHTML()` emits a plain
  // `<code class="language-rust">` — so a published page showed grey code and
  // the "syntax highlighted" label in the block menu was a promise that only
  // held while you were writing. Imported dynamically, and only when the page
  // actually contains code: the library is bigger than this whole view, and
  // most pages are prose.
  useEffect(() => {
    const blocks = ref.current?.querySelectorAll<HTMLElement>("pre code");
    if (!blocks?.length) return;
    let cancelled = false;
    import("highlight.js/lib/common").then(({ default: hljs }) => {
      if (cancelled) return;
      blocks.forEach((el) => {
        // Re-highlighting an element throws in newer versions; the guard is
        // cheaper than the try/catch.
        if (!el.dataset.highlighted) hljs.highlightElement(el);
      });
    });
    return () => {
      cancelled = true;
    };
  }, [html]);

  useEffect(() => {
    const el = ref.current;
    if (!el) return;
    const onClick = (e: MouseEvent) => {
      const anchor = (e.target as HTMLElement).closest("a");
      const href = anchor?.getAttribute("href");
      if (!href?.startsWith("/knowledge/")) return;
      // Let the browser handle modified clicks — "open in a new tab" on a wiki
      // link is a thing people do constantly.
      if (e.metaKey || e.ctrlKey || e.shiftKey || e.button !== 0) return;
      e.preventDefault();
      navigate(href);
    };
    el.addEventListener("click", onClick);
    return () => el.removeEventListener("click", onClick);
  }, [navigate]);

  return <div ref={ref} className="kb" dangerouslySetInnerHTML={{ __html: html }} />;
}
