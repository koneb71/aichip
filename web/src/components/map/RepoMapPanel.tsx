import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { motion } from "framer-motion";
import { api, RepoGraph, RepoIndexStatus, RepoSearchHit } from "../../lib/api";
import { Icon } from "../ui/Icon";
import { moduleOf } from "../../lib/repoGraph";
import { GraphCanvas } from "./GraphCanvas";
import { FileInspector } from "./FileInspector";
import { tintFor } from "./GraphNodes";

/**
 * What this project is made of, read from the code itself.
 *
 * Three ways in, and they answer different questions. The search box answers
 * "where does the thing that does X live" — which Grep cannot, because Grep
 * needs the word and the whole problem is not knowing it. The graph answers
 * "what is this shaped like, and what would I break". The list answers the
 * same as the graph without needing a mouse or an eye for geometry.
 *
 * Everything here is derived and never authored: the Brain is the paragraph a
 * person writes, this is read out of the files. When the two disagree the code
 * is what is true, which is why the strip names the commit it was read at.
 */
export function RepoMapPanel({
  projectId,
  onOpenFile,
}: {
  projectId: string;
  /** Hands a path to the Files tab. The page switches tabs, not this panel. */
  onOpenFile?: (path: string) => void;
}) {
  const [status, setStatus] = useState<RepoIndexStatus | null>(null);
  const [graph, setGraph] = useState<RepoGraph | null>(null);
  const [view, setView] = useState<"graph" | "list">("graph");
  const [selected, setSelected] = useState<string | null>(null);
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<RepoSearchHit[] | null>(null);
  const [hitNote, setHitNote] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refreshStatus = useCallback(async () => {
    try {
      setStatus(await api.repoIndexStatus(projectId));
    } catch {
      /* server restarting; the next poll retries */
    }
  }, [projectId]);

  const refreshGraph = useCallback(async () => {
    try {
      setGraph(await api.repoGraph(projectId));
    } catch {
      /* as above */
    }
  }, [projectId]);

  // Reset first, so switching projects never shows the previous one's files.
  useEffect(() => {
    setStatus(null);
    setGraph(null);
    setSelected(null);
    setHits(null);
    setQuery("");
    setError(null);
    refreshStatus();
    // The graph is not fetched here: the version effect below sees a null
    // graph and does it, and asking twice on mount would race itself.
  }, [refreshStatus, projectId]);

  // Poll while unsettled, stop when settled — the rail's habit. `status !== null`
  // is load-bearing: an unfetched status must not read as busy.
  const unsettled =
    status !== null &&
    (status.phase === "structure" ||
      status.phase === "embedding" ||
      status.embedder.state === "downloading");
  useEffect(() => {
    if (!unsettled) return;
    const t = setInterval(refreshStatus, 2000);
    return () => clearInterval(t);
  }, [unsettled, refreshStatus]);

  // The graph refetches when the *structure* moved, never on the poll tick.
  // Re-deriving nodes every two seconds while embeddings fill in would re-lay
  // out the canvas under the cursor, which is exactly what this counter was
  // added to prevent.
  //
  // `>=` rather than `!==`: a reconcile landing between the two requests can
  // hand back a graph *newer* than the status that asked for it, and equality
  // would then refetch forever.
  const version = status?.structureVersion ?? 0;
  useEffect(() => {
    if (graph && graph.structureVersion >= version) return;
    refreshGraph();
  }, [version, graph, refreshGraph]);

  // Coming back from a terminal or an editor is exactly when the index is most
  // likely to have gone stale, and the status read is what re-triggers it.
  useEffect(() => {
    window.addEventListener("focus", refreshStatus);
    return () => window.removeEventListener("focus", refreshStatus);
  }, [refreshStatus]);

  // Debounced, with a stale guard so a slow response cannot land last.
  const seq = useRef(0);
  useEffect(() => {
    const q = query.trim();
    if (q.length < 2) {
      setHits(null);
      setHitNote(null);
      return;
    }
    const mine = ++seq.current;
    setSearching(true);
    const t = setTimeout(async () => {
      try {
        const r = await api.repoSearch(projectId, q);
        if (mine === seq.current) {
          setHits(r.hits);
          setHitNote(r.note ?? null);
        }
      } catch (e) {
        if (mine === seq.current) {
          setHits([]);
          setHitNote(String(e).replace(/^Error:\s*/, ""));
        }
      } finally {
        // The first search of a session loads the model, which takes a few
        // seconds; the status the box reads is only true once it has.
        if (mine === seq.current) {
          setSearching(false);
          refreshStatus();
        }
      }
    }, 200);
    return () => clearTimeout(t);
  }, [query, projectId, refreshStatus]);

  const reindex = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.reindexRepoMap(projectId);
      await Promise.all([refreshStatus(), refreshGraph()]);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const nodes = useMemo(() => graph?.nodes ?? [], [graph]);
  const edges = useMemo(() => graph?.edges ?? [], [graph]);

  // Grouped by module, largest first — the shape of a project is its modules.
  const groups = useMemo(() => {
    const by = new Map<string, typeof nodes>();
    for (const n of nodes) {
      const key = moduleOf(n.path);
      by.set(key, [...(by.get(key) ?? []), n]);
    }
    return [...by.entries()].sort((a, b) => b[1].length - a[1].length);
  }, [nodes]);

  if (!status) return null;

  // What makes search possible is vectors in the database, not the embedder's
  // state in this process — which is `not_ready` after every restart until
  // something asks it for something. Gating on that would disable the box on a
  // fully indexed project, forever.
  const searchable = status.counts.embedded > 0;
  const dropped = graph ? graph.importsTotal - graph.importsResolved : 0;

  return (
    // `min-w-0` matters: a React Flow pane reports its content width, and
    // without it this flex item widens the whole page into a sideways scroll.
    <div className="flex h-full min-h-0 min-w-0 flex-col">
      <div className="flex items-start gap-3 border-b border-line px-6 py-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-base font-semibold">Map</h2>
          <p className="mt-0.5 max-w-xl text-xs text-ink-dim">
            What this project is made of, read from the code itself and re-read
            whenever it changes. Click a module to open it; click a file to see
            what it would break.
          </p>
        </div>
        <div className="flex shrink-0 rounded-lg border border-line p-0.5">
          {(["graph", "list"] as const).map((v) => (
            <button
              key={v}
              onClick={() => setView(v)}
              className={`ring-focus rounded-md px-2.5 py-1 text-[11px] capitalize transition-colors ${
                view === v ? "bg-panel-2 font-medium" : "text-ink-dim hover:text-ink"
              }`}
            >
              {v}
            </button>
          ))}
        </div>
      </div>

      <div className="border-b border-line px-6 py-2.5">
        <div className="relative max-w-3xl">
          <span className="pointer-events-none absolute left-2.5 top-1/2 -translate-y-1/2 text-ink-dim">
            <Icon name="search" size={14} />
          </span>
          <input
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            placeholder={
              searchable
                ? "Describe what you're looking for — “where do we decide if a card can start”"
                : "Meaning search lights up when the first files are indexed…"
            }
            disabled={!searchable}
            className="ring-focus w-full rounded-xl border border-line bg-panel py-2 pl-8 pr-2.5 text-xs outline-none transition-colors focus:border-accent disabled:opacity-60"
          />
        </div>

        {hits !== null && (
          <div className="mt-2 max-h-60 max-w-3xl space-y-1 overflow-y-auto">
            {searching && hits.length === 0 && (
              <div className="text-[11px] text-ink-dim">Searching…</div>
            )}
            {/* A failed search that renders as "no results" reads as "your
                code does not contain this", which is a different claim. */}
            {!searching && hitNote && (
              <div className="rounded-lg bg-red-50 px-2 py-1.5 text-[11px] text-danger">
                Search did not run — {hitNote}
              </div>
            )}
            {!searching && !hitNote && hits.length === 0 && (
              <div className="text-[11px] text-ink-dim">Nothing matches “{query.trim()}”.</div>
            )}
            {hits.map((h) => (
              <motion.button
                key={`${h.path}:${h.line}`}
                initial={{ opacity: 0, y: 4 }}
                animate={{ opacity: 1, y: 0 }}
                onClick={() => {
                  setSelected(h.path);
                  onOpenFile?.(h.path);
                }}
                className="block w-full rounded-lg border border-line bg-panel px-3 py-2 text-left transition-colors hover:border-ink-dim/40 hover:bg-panel-2"
              >
                <div className="flex items-baseline gap-2">
                  <span className="truncate font-mono text-[11px]">{h.path}</span>
                  {h.line != null && (
                    <span className="shrink-0 text-[10px] text-ink-dim">:{h.line}</span>
                  )}
                  {h.symbol && (
                    <span className="shrink-0 rounded-full bg-panel-2 px-1.5 text-[10px] text-ink-dim">
                      {h.symbol}
                    </span>
                  )}
                  <span className="ml-auto shrink-0 text-[10px] text-ink-dim">
                    {Math.round(h.score * 100)}%
                  </span>
                </div>
                <pre className="mt-1 overflow-hidden whitespace-pre-wrap font-mono text-[10px] leading-snug text-ink-dim">
                  {h.excerpt.split("\n").slice(0, 3).join("\n")}
                </pre>
              </motion.button>
            ))}
          </div>
        )}

        <Banners status={status} error={error} onDismiss={() => setError(null)} />
      </div>

      {/* The canvas is a sibling of any scrolling body, never inside one: an
          `h-full` pane inside an overflow container resolves against scroll
          height, and React Flow then measures a zero-by-zero viewport. */}
      {view === "graph" ? (
        <div className="flex min-h-0 min-w-0 flex-1">
          <GraphCanvas
            files={nodes}
            edges={edges}
            selected={selected}
            onSelect={setSelected}
            onOpenFile={(p) => onOpenFile?.(p)}
          />
          {selected && (
            <FileInspector
              projectId={projectId}
              path={selected}
              onOpenFile={(p) => onOpenFile?.(p)}
              onClose={() => setSelected(null)}
            />
          )}
        </div>
      ) : (
        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-4">
          {status.note && nodes.length === 0 && (
            <div className="rounded-xl border border-dashed border-line px-6 py-10 text-center text-xs text-ink-dim">
              {status.note}
            </div>
          )}
          <div className="max-w-3xl space-y-4">
            {groups.map(([name, group]) => (
              <div key={name}>
                <div className="mb-1.5 flex items-baseline gap-2">
                  <span className="font-mono text-xs font-semibold">{name}</span>
                  <span className="text-[11px] text-ink-dim">
                    {group.length} file{group.length === 1 ? "" : "s"}
                  </span>
                </div>
                <table className="w-full text-[11px]">
                  <thead>
                    <tr className="text-[10px] text-ink-dim">
                      <th className="w-full text-left font-normal">File</th>
                      <th className="px-2 text-right font-normal" title="Files that import this">
                        in
                      </th>
                      <th className="px-2 text-right font-normal" title="Files this imports">
                        out
                      </th>
                      <th className="pl-2 text-right font-normal">defines</th>
                    </tr>
                  </thead>
                  <tbody>
                    {[...group]
                      .sort((a, b) => b.importedBy - a.importedBy || a.path.localeCompare(b.path))
                      .map((f) => (
                        <tr key={f.path} className="hover:bg-panel-2">
                          <td>
                            <button
                              onClick={() => onOpenFile?.(f.path)}
                              title={f.path}
                              className="ring-focus flex w-full items-center gap-1.5 rounded py-0.5 text-left"
                            >
                              <span
                                className="size-1.5 shrink-0 rounded-full"
                                style={{ background: tintFor(f.lang).fg }}
                              />
                              <span className="truncate font-mono">
                                {f.path.slice(name.length + 1) || f.path}
                              </span>
                            </button>
                          </td>
                          <td className="px-2 text-right tabular-nums text-ink-dim">
                            {f.importedBy}
                          </td>
                          <td className="px-2 text-right tabular-nums text-ink-dim">{f.imports}</td>
                          <td className="pl-2 text-right tabular-nums text-ink-dim">{f.symbols}</td>
                        </tr>
                      ))}
                  </tbody>
                </table>
              </div>
            ))}
          </div>
        </div>
      )}

      {/* The steady-state strip: ambient, not primary. */}
      <div className="flex items-center gap-2 border-t border-line px-6 py-1.5 text-[11px] text-ink-dim">
        <span
          className={`size-1.5 rounded-full ${
            unsettled ? "animate-pulse bg-tier-medium" : "bg-tier-easy"
          }`}
        />
        <span>{phaseLine(status)}</span>
        {graph && (
          <span title="Every edge is one file importing another, resolved against this project">
            · {graph.edges.length} links
          </span>
        )}
        {/* An import that resolves to nothing draws no edge, and a node with no
            edges reads as "safe to delete". Saying how many were dropped is
            what lets somebody weigh an empty neighbourhood. */}
        {dropped > 0 && (
          <span title="Specifiers naming a package, or naming nothing in this project. Dropped rather than guessed at.">
            · {dropped} outside
          </span>
        )}
        {status.indexedSha && (
          <span className="font-mono" title="The commit this was read at">
            · {status.indexedSha.slice(0, 7)}
          </span>
        )}
        <span className="ml-auto flex items-center gap-3">
          <span className="hidden sm:inline" title="Node size is how many files import it">
            size = imported-by
          </span>
          <button
            onClick={reindex}
            disabled={busy}
            title="Read the project again and rebuild the index"
            className="ring-focus text-ink-dim hover:text-ink disabled:opacity-50"
          >
            {busy ? "Reading…" : "↻ Reindex"}
          </button>
        </span>
      </div>
    </div>
  );
}

/** Abnormal and transient states only. */
function Banners({
  status,
  error,
  onDismiss,
}: {
  status: RepoIndexStatus;
  error: string | null;
  onDismiss: () => void;
}) {
  return (
    <>
      {/* "Loading" covers both truths: the very first time this downloads
          ~35 MB, and every run after that it is reading the cached model
          off disk, which still takes a few seconds. */}
      {status.embedder.state === "downloading" && (
        <div className="mt-2 max-w-3xl rounded-lg bg-amber-50 px-2 py-1.5 text-[11px] text-amber-800">
          Loading the embedding model — a few seconds, and ~35 MB the first time
          ever. The map below works now.
        </div>
      )}
      {status.embedder.state === "failed" && (
        <div
          className="mt-2 max-w-3xl rounded-lg bg-red-50 px-2 py-1.5 text-[11px] text-danger"
          title={status.embedder.detail}
        >
          The embedder failed — indexing retries on the next pass. The map still
          works, and agents still have Read and Grep.
        </div>
      )}
      {status.error && (
        <div className="mt-2 max-w-3xl rounded-lg bg-red-50 px-2 py-1.5 text-[11px] text-danger">
          Reading the project failed — {status.error.split("\n").slice(-1)[0]}
        </div>
      )}
      {error && (
        <button
          onClick={onDismiss}
          title="Dismiss"
          className="mt-2 block w-full max-w-3xl rounded-lg bg-red-50 px-2 py-1.5 text-left text-[11px] text-danger"
        >
          {error}
        </button>
      )}
    </>
  );
}

/**
 * What the strip says, and the wording matters most in the middle state:
 * once the files are read the map is complete and correct, and only *search*
 * is still filling in. Saying "indexing…" through both phases would claim
 * otherwise.
 */
function phaseLine(s: RepoIndexStatus): string {
  switch (s.phase) {
    case "never":
      return "Nothing read yet — this starts when the project opens.";
    case "structure":
      return `Reading the project — ${s.counts.parsed} of ${s.counts.files} files.`;
    case "embedding":
      return `Read. Adding meaning search — ${s.counts.embedded} of ${s.counts.files}.`;
    case "failed":
      return "The last read failed — what was found before still stands.";
    default:
      return `${s.counts.files} files · ${s.counts.embedded} searchable`;
  }
}
