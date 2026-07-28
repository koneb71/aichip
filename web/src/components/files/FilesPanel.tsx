import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, FileContent, FileEntry, FileListing } from "../../lib/api";
import { NARROW, useMediaQuery } from "../../lib/useMediaQuery";

/** Read-only browser for the project checkout: tree on the left, file on the
 *  right — or one at a time on a narrow viewport. */
export function FilesPanel({ projectId }: { projectId: string }) {
  const narrow = useMediaQuery(NARROW);
  const [listing, setListing] = useState<FileListing | null>(null);
  const [dir, setDir] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [file, setFile] = useState<FileContent | null>(null);
  const [loadingFile, setLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // Reset when switching projects, or we'd browse the old tree's paths.
  useEffect(() => {
    setDir("");
    setSelected(null);
    setFile(null);
  }, [projectId]);

  useEffect(() => {
    let stale = false;
    setError(null);
    api
      .files(projectId, dir)
      .then((l) => {
        if (!stale) setListing(l);
      })
      .catch((e) => {
        if (!stale) setError(String(e));
      });
    return () => {
      stale = true;
    };
  }, [projectId, dir]);

  const open = useCallback(
    (entry: FileEntry) => {
      if (entry.kind === "dir") {
        setDir(entry.path);
        return;
      }
      setSelected(entry.path);
      setLoadingFile(true);
      setFile(null);
      api
        .file(projectId, entry.path)
        .then(setFile)
        .catch((e) => setError(String(e)))
        .finally(() => setLoadingFile(false));
    },
    [projectId],
  );

  // Below `lg` the tree and the viewer take turns: opening a file swaps to it,
  // and the viewer offers a way back. Two 50%-wide panes would make both the
  // paths and the code unreadable.
  const showTree = !narrow || !selected;
  const showViewer = !narrow || !!selected;

  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)] lg:grid-cols-[280px_minmax(0,1fr)]">
      <div
        className={`${showTree ? "flex" : "hidden"} min-h-0 min-w-0 flex-col border-line bg-panel lg:flex lg:border-r`}
      >
        <Breadcrumbs dir={dir} onNavigate={setDir} />
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {listing?.parent !== null && listing !== null && (
            <button
              onClick={() => setDir(listing.parent ?? "")}
              className="flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm text-ink-dim hover:bg-panel-2"
            >
              <span className="w-4 text-center">↰</span> ..
            </button>
          )}
          {listing?.entries.map((entry) => (
            <button
              key={entry.path}
              onClick={() => open(entry)}
              className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left text-sm ${
                selected === entry.path
                  ? "bg-panel-2 font-medium text-ink"
                  : "text-ink-dim hover:bg-panel-2 hover:text-ink"
              }`}
            >
              <span className="w-4 text-center">{entry.kind === "dir" ? "▸" : "·"}</span>
              <span className="truncate">{entry.name}</span>
              {entry.size !== null && (
                <span className="ml-auto shrink-0 text-[10px] text-ink-dim/70">
                  {humanSize(entry.size)}
                </span>
              )}
            </button>
          ))}
          {listing?.entries.length === 0 && (
            <div className="px-2 py-3 text-xs text-ink-dim">Empty folder.</div>
          )}
        </div>
      </div>

      <div className={`${showViewer ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}>
        {error && (
          <div className="m-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}
        {!selected && !error && (
          <div className="mt-16 px-6 text-center text-sm text-ink-dim">
            Select a file to view it. This is a read-only view of your checkout —
            changes happen in task worktrees.
          </div>
        )}
        {selected && (
          <>
            <div className="flex items-center gap-2 border-b border-line bg-panel px-4 py-2">
              {narrow && (
                <button
                  onClick={() => setSelected(null)}
                  className="shrink-0 rounded-md px-1.5 py-0.5 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
                >
                  ← Files
                </button>
              )}
              <div className="truncate font-mono text-xs text-ink-dim">{selected}</div>
              {file && !file.tooLarge && (
                <span className="ml-auto shrink-0 text-[10px] text-ink-dim/70">
                  {humanSize(file.size)}
                </span>
              )}
            </div>
            <div className="min-h-0 flex-1 overflow-auto">
              {loadingFile && <div className="p-4 text-xs text-ink-dim">Loading…</div>}
              {file?.tooLarge && (
                <div className="p-4 text-sm text-ink-dim">
                  This file is {humanSize(file.size)} — too large to preview.
                </div>
              )}
              {file?.binary && (
                <div className="p-4 text-sm text-ink-dim">
                  Binary file ({humanSize(file.size)}), not shown.
                </div>
              )}
              {file?.content !== null && file?.content !== undefined && (
                <motion.pre
                  initial={{ opacity: 0 }}
                  animate={{ opacity: 1 }}
                  className="p-4 font-mono text-xs leading-relaxed whitespace-pre"
                >
                  {file.content}
                </motion.pre>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Breadcrumbs({ dir, onNavigate }: { dir: string; onNavigate: (p: string) => void }) {
  const parts = dir ? dir.split("/") : [];
  return (
    <div className="flex flex-wrap items-center gap-1 border-b border-line px-3 py-2 text-xs">
      <button
        onClick={() => onNavigate("")}
        className={parts.length === 0 ? "font-medium text-ink" : "text-ink-dim hover:text-ink"}
      >
        root
      </button>
      {parts.map((part, i) => (
        <span key={i} className="flex items-center gap-1">
          <span className="text-ink-dim/50">/</span>
          <button
            onClick={() => onNavigate(parts.slice(0, i + 1).join("/"))}
            className={
              i === parts.length - 1 ? "font-medium text-ink" : "text-ink-dim hover:text-ink"
            }
          >
            {part}
          </button>
        </span>
      ))}
    </div>
  );
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
