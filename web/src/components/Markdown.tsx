import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

/** Markdown for chat bubbles and stream text. react-markdown never injects
 *  raw HTML, so model output stays safe to render. */
export function Markdown({ children }: { children: string }) {
  return (
    <div className="md">
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={MD_COMPONENTS}>
        {children}
      </ReactMarkdown>
    </div>
  );
}

/** A wide table has to scroll on its own; left bare it widens whatever column
 *  it sits in until a sibling panel gets pushed out. */
const MD_COMPONENTS = {
  table: ({ children, ...props }: React.ComponentPropsWithoutRef<"table">) => (
    <div className="table-scroll">
      <table {...props}>{children}</table>
    </div>
  ),
};
