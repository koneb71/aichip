import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, RepoFileDetail } from "../../lib/api";

/**
 * What one file is, once you have picked it out of the graph.
 *
 * The importers come first and that is the point: "what would I break" is the
 * question a person opens a dependency graph to ask, and a panel that leads
 * with the file's own definitions answers a different, easier one.
 *
 * Fetched on selection rather than shipped with the graph — this repository's
 * 590 symbols are 50KB nobody looks at until they click.
 */
export function FileInspector({
  projectId,
  path,
  onOpenFile,
  onClose,
}: {
  projectId: string;
  path: string;
  onOpenFile: (path: string) => void;
  onClose: () => void;
}) {
  const [detail, setDetail] = useState<RepoFileDetail | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let live = true;
    setDetail(null);
    setError(null);
    api
      .repoFile(projectId, path)
      .then((d) => live && setDetail(d))
      .catch((e) => live && setError(String(e).replace(/^Error:\s*/, "")));
    return () => {
      live = false;
    };
  }, [projectId, path]);

  return (
    <motion.aside
      initial={{ opacity: 0, x: 8 }}
      animate={{ opacity: 1, x: 0 }}
      transition={{ duration: 0.15, ease: "easeOut" }}
      className="flex w-72 shrink-0 flex-col overflow-y-auto border-l border-line bg-panel"
    >
      <div className="flex items-start gap-2 border-b border-line px-3 py-2">
        <div className="min-w-0 flex-1">
          <div className="break-all font-mono text-[11px] font-semibold">{path}</div>
        </div>
        <button
          onClick={onClose}
          title="Close"
          className="ring-focus rounded px-1 text-xs text-ink-dim hover:text-ink"
        >
          ✕
        </button>
      </div>

      <div className="flex gap-1.5 border-b border-line px-3 py-2">
        <button
          onClick={() => onOpenFile(path)}
          className="ring-focus rounded-lg border border-line px-2 py-1 text-[11px] transition-colors hover:bg-panel-2"
        >
          Open in Files
        </button>
      </div>

      {error && <div className="px-3 py-3 text-[11px] text-danger">{error}</div>}
      {!detail && !error && <div className="px-3 py-3 text-[11px] text-ink-dim">Reading…</div>}

      {detail && (
        <div className="space-y-4 px-3 py-3">
          <Section
            title="Imported by"
            count={detail.importers.length}
            empty="Nothing in this project imports it."
          >
            {detail.importers.map((i) => (
              <Row key={i.path} path={i.path} weight={i.weight} onOpen={() => onOpenFile(i.path)} />
            ))}
          </Section>

          <Section title="Imports" count={detail.imports.length} empty="It imports nothing here.">
            {detail.imports.map((i) => (
              <Row key={i.path} path={i.path} weight={i.weight} onOpen={() => onOpenFile(i.path)} />
            ))}
          </Section>

          <Section
            title="Defines"
            count={detail.symbols.length}
            empty="No definitions — either it has none, or no grammar here reads this language."
          >
            {detail.symbols.map((s) => (
              <div
                key={`${s.name}:${s.line}`}
                title={s.signature ?? undefined}
                className="flex items-baseline gap-1.5 rounded px-1 py-0.5 text-[11px]"
              >
                <span className="truncate font-mono">{s.name}</span>
                <span className="shrink-0 rounded-full bg-panel-2 px-1 text-[9px] text-ink-dim">
                  {s.kind}
                </span>
                <span className="ml-auto shrink-0 font-mono text-[10px] text-ink-dim">
                  :{s.line}
                </span>
              </div>
            ))}
          </Section>

          {/* Named rather than hidden: an external package and a broken
              relative path look identical once the edge has been dropped, and
              only one of them is a problem. */}
          {detail.specifiers.length > 0 && (
            <details className="text-[11px]">
              <summary className="cursor-pointer text-ink-dim">
                All {detail.specifiers.length} specifiers as written
              </summary>
              <div className="mt-1 space-y-0.5">
                {detail.specifiers.map((s) => (
                  <div key={s} className="truncate px-1 font-mono text-[10px] text-ink-dim">
                    {s}
                  </div>
                ))}
              </div>
            </details>
          )}
        </div>
      )}
    </motion.aside>
  );
}

function Section({
  title,
  count,
  empty,
  children,
}: {
  title: string;
  count: number;
  empty: string;
  children: React.ReactNode;
}) {
  return (
    <div>
      <div className="mb-1 flex items-baseline gap-1.5">
        <span className="text-[11px] font-semibold">{title}</span>
        <span className="text-[10px] text-ink-dim">{count}</span>
      </div>
      {count === 0 ? <div className="text-[10px] text-ink-dim">{empty}</div> : children}
    </div>
  );
}

function Row({
  path,
  weight,
  onOpen,
}: {
  path: string;
  weight: number;
  onOpen: () => void;
}) {
  return (
    <button
      onClick={onOpen}
      title={path}
      className="ring-focus flex w-full items-baseline gap-1.5 rounded px-1 py-0.5 text-left text-[11px] hover:bg-panel-2"
    >
      <span className="truncate font-mono">{path}</span>
      {weight > 1 && (
        <span className="ml-auto shrink-0 text-[10px] text-ink-dim">×{weight}</span>
      )}
    </button>
  );
}
