import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Markdown for chat bubbles and stream text. react-markdown never injects
 *  raw HTML, so model output stays safe to render. */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]}>{children}</ReactMarkdown>
    </div>
  );
}
