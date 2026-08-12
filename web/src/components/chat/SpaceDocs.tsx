import { useCallback, useEffect, useRef, useState } from "react";
import { api, SpaceDocument, SpaceDocsStatus } from "../../lib/api";

/**
 * The documents in a space, with their semantic-index status.
 *
 * Lives in the Chat page's rail: drop files in, watch them index, and the
 * space's chat retrieves from them. Polls only while something is unsettled —
 * a pending document or a downloading embedder — and stops when the dust
 * settles, matching the "no idle polling" habit of the rest of the rail.
 */
export function SpaceDocs({ projectId }: { projectId: string }) {
  const [docs, setDocs] = useState<SpaceDocument[]>([]);
  const [status, setStatus] = useState<SpaceDocsStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const fileRef = useRef<HTMLInputElement>(null);

  const refresh = useCallback(async () => {
    try {
      const [d, s] = await Promise.all([
        api.spaceDocuments(projectId),
        api.spaceDocsStatus(projectId),
      ]);
      setDocs(d.documents);
      setStatus(s);
    } catch {
      /* server restarting; the next poll retries */
    }
  }, [projectId]);

  useEffect(() => {
    setDocs([]);
    setStatus(null);
    setError(null);
    refresh();
  }, [refresh]);

  // Poll while unsettled, stop when settled.
  const unsettled =
    docs.some((d) => d.status === "pending") ||
    status?.embedder.state === "downloading";
  useEffect(() => {
    if (!unsettled) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [unsettled, refresh]);

  const upload = async (files: File[]) => {
    if (files.length === 0 || busy) return;
    setBusy(true);
    setError(null);
    try {
      await api.uploadSpaceDocuments(projectId, files);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const remove = async (id: string) => {
    try {
      await api.deleteSpaceDocument(projectId, id);
      refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  const reindex = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.reindexSpace(projectId);
      refresh();
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="flex min-h-0 flex-col gap-1.5 border-t border-line pt-3">
      <div className="flex items-center justify-between px-1">
        <span className="text-xs font-medium text-ink-dim">Documents</span>
        <button
          onClick={reindex}
          disabled={busy}
          title="Re-scan the folder and refresh the search index"
          className="text-[11px] text-ink-dim hover:text-ink disabled:opacity-50"
        >
          ↻ Reindex
        </button>
      </div>

      {status?.embedder.state === "downloading" && (
        <div className="rounded-lg bg-amber-50 px-2 py-1.5 text-[11px] text-amber-800">
          Downloading the embedding model — one-time, ~35 MB…
        </div>
      )}
      {status?.embedder.state === "failed" && (
        <div
          className="rounded-lg bg-red-50 px-2 py-1.5 text-[11px] text-danger"
          title={status.embedder.detail}
        >
          The embedder failed — indexing will retry. Chat still works via Read.
        </div>
      )}
      {error && (
        <button
          onClick={() => setError(null)}
          className="rounded-lg bg-red-50 px-2 py-1.5 text-left text-[11px] text-danger"
          title="Dismiss"
        >
          {error}
        </button>
      )}

      <div
        onDragOver={(e) => {
          e.preventDefault();
          setDragging(true);
        }}
        onDragLeave={() => setDragging(false)}
        onDrop={(e) => {
          e.preventDefault();
          setDragging(false);
          upload(Array.from(e.dataTransfer.files));
        }}
        onClick={() => fileRef.current?.click()}
        className={`cursor-pointer rounded-lg border border-dashed px-2 py-2 text-center text-[11px] ${
          dragging ? "border-accent bg-accent/5 text-accent" : "border-line text-ink-dim hover:border-ink-dim"
        }`}
      >
        {busy ? "Uploading…" : "Drop documents here, or click to pick"}
        <input
          ref={fileRef}
          type="file"
          multiple
          accept=".md,.txt,.csv,.json,.log,.pdf"
          className="hidden"
          onChange={(e) => {
            upload(Array.from(e.target.files ?? []));
            e.target.value = "";
          }}
        />
      </div>

      <div className="min-h-0 overflow-y-auto">
        {docs.map((d) => (
          <div
            key={d.id}
            className="group flex items-center gap-1.5 rounded-lg px-1.5 py-1 text-xs hover:bg-panel-2"
          >
            <StatusDot doc={d} />
            <span className="min-w-0 flex-1 truncate" title={d.relPath}>
              {d.relPath}
            </span>
            <button
              onClick={() => remove(d.id)}
              title="Delete this document"
              className="shrink-0 px-1 text-[11px] text-ink-dim opacity-0 hover:text-danger group-hover:opacity-100"
            >
              ✕
            </button>
          </div>
        ))}
        {docs.length === 0 && (
          <div className="px-1.5 py-1 text-[11px] text-ink-dim">
            No documents yet. The chat retrieves from what you drop here.
          </div>
        )}
      </div>
    </div>
  );
}

function StatusDot({ doc }: { doc: SpaceDocument }) {
  const [color, title] = (() => {
    switch (doc.status) {
      case "indexed":
        return ["bg-tier-easy", "indexed — searchable"];
      case "pending":
        return ["bg-tier-medium animate-pulse", "indexing…"];
      case "failed":
        return ["bg-danger", doc.error ?? "indexing failed"];
      default:
        return ["bg-ink-dim/40", "readable by the assistant, not searchable"];
    }
  })();
  return <span title={title} className={`h-1.5 w-1.5 shrink-0 rounded-full ${color}`} />;
}
