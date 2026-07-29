import { useEffect, useState } from "react";

interface Heading {
  id: string;
  text: string;
  level: number;
}

/**
 * Contents, scraped from the rendered body.
 *
 * Read from the DOM rather than written into `content_html` on purpose: a
 * stored table of contents goes stale the moment an agent rewrites the page,
 * and would then appear in the diff as a change nobody made.
 */
export function PageToc({ html }: { html: string }) {
  const [headings, setHeadings] = useState<Heading[]>([]);

  useEffect(() => {
    const doc = new DOMParser().parseFromString(html, "text/html");
    const found: Heading[] = [];
    doc.querySelectorAll("h2, h3").forEach((el, i) => {
      const text = el.textContent?.trim();
      if (!text) return;
      found.push({ id: `kb-h-${i}`, text, level: el.tagName === "H2" ? 2 : 3 });
    });
    setHeadings(found);

    // Stamp the same ids onto the live nodes so the links land somewhere.
    document
      .querySelectorAll(".kb h2, .kb h3")
      .forEach((el, i) => el.setAttribute("id", `kb-h-${i}`));
  }, [html]);

  // One heading is not a structure; showing a contents list for it is noise.
  if (headings.length < 2) return null;

  return (
    <nav className="sticky top-6">
      <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
        On this page
      </div>
      <ul className="space-y-1 border-l border-line">
        {headings.map((h) => (
          <li key={h.id} style={{ paddingLeft: h.level === 3 ? 20 : 10 }}>
            <a
              href={`#${h.id}`}
              onClick={(e) => {
                e.preventDefault();
                document.getElementById(h.id)?.scrollIntoView({ behavior: "smooth" });
              }}
              className="block truncate text-xs text-ink-dim hover:text-accent"
            >
              {h.text}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}
