import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api } from "../lib/api";
import {
  agentMatches,
  applyAgentMention,
  applyMention,
  LineSpec,
  mentionToken,
  parseLineSpec,
} from "../lib/mention";

interface FileHit {
  path: string;
  name: string;
}

/** Just enough of an agent to offer it and draw it. */
export interface AgentHit {
  id: string;
  name: string;
  icon: string;
  color: string;
  description: string;
}

/** Agents shown at once. The list is who you employ, not what you have —
 *  a handful, and the query narrows it. */
const AGENT_LIMIT = 6;

/** One offer in the picker. Agents and files share a cursor because they
 *  share one Enter key. */
type Row = { kind: "agent"; agent: AgentHit } | { kind: "file"; hit: FileHit };

/** Lines rendered at once. A 20k-line file must not become 20k DOM nodes. */
const LINE_WINDOW = 300;

/**
 * `@`-mention picker: agents from the library, then project files, with an
 * optional line/line-range step on a file.
 *
 * Returned as a hook rather than a plain component because the composer has to
 * hand it keystrokes *before* acting on them — otherwise Enter sends the
 * message instead of choosing a file.
 *
 * Agents come first because they are the smaller and more deliberate choice: a
 * file mention is context for the request, an agent mention decides who does
 * the work.
 */
export function useMentionPicker({
  projectId,
  agents,
  text,
  caret,
  onApply,
}: {
  projectId: string;
  /** The workspace's agents. Passed in rather than fetched here, so the
   *  composer and the message bubbles resolve mentions against one list. */
  agents: AgentHit[];
  text: string;
  caret: number;
  /** Called with the rewritten text and where to put the caret. */
  onApply: (text: string, caret: number) => void;
}) {
  const token = useMemo(() => mentionToken(text, caret), [text, caret]);
  const [dismissed, setDismissed] = useState<string | null>(null);
  const [files, setFiles] = useState<FileHit[]>([]);
  const [truncated, setTruncated] = useState(false);
  const [rawCursor, setCursor] = useState(0);

  // Line mode: set once a file is chosen with ':' or typed with a suffix.
  const [linePath, setLinePath] = useState<string | null>(null);
  const [lines, setLines] = useState<string[] | null>(null);
  const [lineNote, setLineNote] = useState<string | null>(null);
  const [anchor, setAnchor] = useState<number | null>(null);
  const [lineCursor, setLineCursor] = useState(0);

  // Escape dismisses only the current token, so typing on reopens it.
  const tokenKey = token ? `${token.start}:${token.query}` : null;
  const open = !!token && dismissed !== tokenKey;
  const inLineMode = open && linePath !== null;

  const reset = useCallback(() => {
    setLinePath(null);
    setLines(null);
    setLineNote(null);
    setAnchor(null);
    setLineCursor(0);
  }, []);

  // Typing `:` after a path enters line mode without touching the mouse.
  useEffect(() => {
    if (!open || !token) return;
    if (token.lineQuery !== null && linePath === null && token.query) {
      setLinePath(token.query);
    }
    if (token.lineQuery === null && linePath !== null) {
      reset();
    }
  }, [open, token, linePath, reset]);

  // Back to the top whenever the query changes.
  //
  // On the token rather than on the search result: the agent rows above the
  // files are filtered synchronously, so resetting when the *answer* arrived
  // moved the highlight 180ms after the user had already started arrowing
  // through them.
  useEffect(() => {
    setCursor(0);
  }, [tokenKey]);

  // Debounced file search. `stale` guards a slow response landing last.
  useEffect(() => {
    if (!open || !token || inLineMode) return;
    let stale = false;
    const timer = setTimeout(() => {
      api
        .searchFiles(projectId, token.query || ".")
        .then((r) => {
          if (stale) return;
          setFiles(r.files);
          setTruncated(r.truncated);
        })
        .catch(() => {
          if (!stale) setFiles([]);
        });
    }, 180);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [projectId, open, token, inLineMode]);

  // Fetch the chosen file once, to show its lines.
  useEffect(() => {
    if (!linePath) return;
    let stale = false;
    setLines(null);
    setLineNote(null);
    api
      .file({ kind: "project", id: projectId }, linePath)
      .then((f) => {
        if (stale) return;
        // Nothing to point at in a binary or oversized file.
        if (f.binary) setLineNote("Binary file — inserting the path only.");
        else if (f.tooLarge) setLineNote("File is too large to show lines — inserting the path only.");
        else setLines((f.content ?? "").split("\n"));
      })
      .catch(() => {
        if (!stale) setLineNote("Could not read the file — inserting the path only.");
      });
    return () => {
      stale = true;
    };
  }, [projectId, linePath]);

  // A typed suffix (`@a.ts:10-25`) drives the highlight too.
  const typedSpec = token?.lineQuery ? parseLineSpec(token.lineQuery) : null;
  useEffect(() => {
    if (typedSpec) setLineCursor(typedSpec.start - 1);
  }, [typedSpec?.start, typedSpec?.end]);

  const commit = useCallback(
    (path: string, spec?: LineSpec) => {
      if (!token) return;
      const { text: next, caret: nextCaret } = applyMention(
        text,
        token.start,
        caret,
        path,
        spec,
      );
      onApply(next, nextCaret);
      reset();
      setDismissed(null);
    },
    [token, text, caret, onApply, reset],
  );

  const commitAgent = useCallback(
    (name: string) => {
      if (!token) return;
      const { text: next, caret: nextCaret } = applyAgentMention(
        text,
        token.start,
        caret,
        name,
      );
      onApply(next, nextCaret);
      reset();
      setDismissed(null);
    },
    [token, text, caret, onApply, reset],
  );

  // Agents are matched here rather than fetched: the library is small and
  // already in memory, so there is nothing to debounce and no empty frame
  // between typing and seeing who is available.
  const agentHits = useMemo(
    () =>
      inLineMode || !token
        ? []
        : agents.filter((a) => agentMatches(token.query, a.name)).slice(0, AGENT_LIMIT),
    [agents, token, inLineMode],
  );

  const rows: Row[] = useMemo(
    () => [
      ...agentHits.map((agent) => ({ kind: "agent" as const, agent })),
      ...files.map((hit) => ({ kind: "file" as const, hit })),
    ],
    [agentHits, files],
  );

  // Keep the highlight inside the list.
  //
  // It used to be enough that the only thing that changed `files` also called
  // `setCursor(0)`. Agents broke that: they are filtered synchronously on every
  // keystroke while the file search is still 180ms behind, so a list the cursor
  // was pointing into can shrink under it between renders. An out-of-range
  // cursor made `handleKey` return false for Enter — and the composer's
  // fall-through for an unhandled Enter is to *send the message*.
  const cursor = Math.min(rawCursor, Math.max(0, rows.length - 1));

  /**
   * Consume a keystroke. Returns true when the picker handled it, in which
   * case the composer must not also act on it.
   */
  const handleKey = useCallback(
    (e: React.KeyboardEvent): boolean => {
      if (!open) return false;

      if (e.key === "Escape") {
        // Back out of line mode first, so a mistyped ':' isn't a dead end.
        if (inLineMode) reset();
        else setDismissed(tokenKey);
        return true;
      }

      if (inLineMode) {
        if (!lines) return false; // still loading, or degraded to path-only
        if (e.key === "ArrowDown" || e.key === "ArrowUp") {
          const delta = e.key === "ArrowDown" ? 1 : -1;
          setLineCursor((c) => Math.min(lines.length - 1, Math.max(0, c + delta)));
          if (e.shiftKey && anchor === null) setAnchor(lineCursor);
          return true;
        }
        if (e.key === "Enter") {
          const start = Math.min(anchor ?? lineCursor, lineCursor) + 1;
          const end = Math.max(anchor ?? lineCursor, lineCursor) + 1;
          // Shift+Enter starts a range from here rather than committing.
          if (e.shiftKey && anchor === null) {
            setAnchor(lineCursor);
            return true;
          }
          commit(linePath!, start === end ? { start } : { start, end });
          return true;
        }
        return false;
      }

      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        if (!rows.length) return false;
        const delta = e.key === "ArrowDown" ? 1 : -1;
        // From the clamped cursor, not the stored one: wrapping a stale index
        // that is past the end lands somewhere the user was not looking.
        setCursor((cursor + delta + rows.length) % rows.length);
        return true;
      }
      if (e.key === "Enter") {
        const row = rows[cursor];
        // Only reachable with nothing to offer, where letting Enter through to
        // send the message is the right answer.
        if (!row) return false;
        if (row.kind === "agent") commitAgent(row.agent.name);
        else commit(row.hit.path);
        return true;
      }
      // ':' jumps straight to picking lines. Only a file has lines, so it
      // takes the highlighted row when that is a file and otherwise the first
      // one — without the fallback, having any agent match the query put the
      // cursor on an agent and silently disabled the shortcut.
      if (e.key === ":") {
        const row = rows[cursor];
        const file = row?.kind === "file" ? row.hit : rows.find((r) => r.kind === "file")?.hit;
        if (file) {
          e.preventDefault();
          setLinePath(file.path);
          return true;
        }
      }
      return false;
    },
    [
      open,
      inLineMode,
      lines,
      lineCursor,
      anchor,
      rows,
      cursor,
      commit,
      commitAgent,
      linePath,
      tokenKey,
      reset,
    ],
  );

  // Once a file turns out to have no showable lines, insert the bare path.
  useEffect(() => {
    if (lineNote && linePath) {
      const t = setTimeout(() => commit(linePath), 900);
      return () => clearTimeout(t);
    }
  }, [lineNote, linePath, commit]);

  const node = (
    <AnimatePresence>
      {open && (
        <motion.div
          initial={{ opacity: 0, y: -4 }}
          animate={{ opacity: 1, y: 0 }}
          exit={{ opacity: 0, y: -4 }}
          className="absolute bottom-full left-0 right-0 z-20 mb-1 max-h-72 overflow-y-auto rounded-xl border border-line bg-panel p-1 shadow-lg"
        >
          {inLineMode ? (
            <LineList
              path={linePath!}
              lines={lines}
              note={lineNote}
              cursor={lineCursor}
              anchor={anchor}
              onPick={(i) => commit(linePath!, { start: i + 1 })}
            />
          ) : (
            <OfferList
              rows={rows}
              cursor={cursor}
              truncated={truncated}
              query={token?.query ?? ""}
              onHover={setCursor}
              onPickAgent={(agent) => commitAgent(agent.name)}
              onPick={(hit) => commit(hit.path)}
              onPickLines={(hit) => setLinePath(hit.path)}
            />
          )}
        </motion.div>
      )}
    </AnimatePresence>
  );

  return { open, handleKey, node };
}

function OfferList({
  rows,
  cursor,
  truncated,
  query,
  onHover,
  onPickAgent,
  onPick,
  onPickLines,
}: {
  rows: Row[];
  cursor: number;
  truncated: boolean;
  query: string;
  onHover: (i: number) => void;
  onPickAgent: (agent: AgentHit) => void;
  onPick: (hit: FileHit) => void;
  onPickLines: (hit: FileHit) => void;
}) {
  if (!rows.length) {
    return (
      <div className="px-2 py-3 text-xs text-ink-dim">
        {query ? `Nothing matches “${query}”.` : "Type to find an agent or a file."}
      </div>
    );
  }
  const agents = rows.filter((r) => r.kind === "agent").length;
  return (
    <>
      {rows.map((row, i) => (
        <div key={row.kind === "agent" ? `a:${row.agent.id}` : `f:${row.hit.path}`}>
          {/* Headers hang off the first row of each kind rather than being
              rows themselves — the cursor indexes this list, and a header it
              could land on would be a dead Enter. */}
          {i === 0 && row.kind === "agent" && <Heading>Assign to</Heading>}
          {i === agents && row.kind === "file" && agents > 0 && <Heading>Files</Heading>}
          {row.kind === "agent" ? (
            <button
              onMouseEnter={() => onHover(i)}
              onMouseDown={(e) => {
                e.preventDefault(); // keep focus in the composer
                onPickAgent(row.agent);
              }}
              className={`flex w-full items-center gap-2 rounded-lg px-2 py-1.5 text-left ${
                i === cursor ? "bg-panel-2" : ""
              }`}
            >
              <span
                className="size-2 shrink-0 rounded-full"
                style={{ background: row.agent.color }}
              />
              <span className="min-w-0 flex-1">
                <span className="block truncate text-sm font-medium">{row.agent.name}</span>
                {row.agent.description && (
                  <span className="block truncate text-[11px] text-ink-dim">
                    {row.agent.description}
                  </span>
                )}
              </span>
            </button>
          ) : (
            <div
              onMouseEnter={() => onHover(i)}
              className={`flex items-center gap-2 rounded-lg px-2 py-1.5 ${
                i === cursor ? "bg-panel-2" : ""
              }`}
            >
              <button
                onMouseDown={(e) => {
                  e.preventDefault();
                  onPick(row.hit);
                }}
                className="flex min-w-0 flex-1 flex-col text-left"
              >
                <span className="truncate text-sm">{row.hit.name}</span>
                <span className="truncate text-[11px] text-ink-dim">{row.hit.path}</span>
              </button>
              <button
                onMouseDown={(e) => {
                  e.preventDefault();
                  onPickLines(row.hit);
                }}
                title="Pick specific lines"
                className="shrink-0 rounded px-1.5 py-0.5 text-[10px] text-ink-dim hover:bg-panel hover:text-ink"
              >
                :lines
              </button>
            </div>
          )}
        </div>
      ))}
      {truncated && (
        <div className="px-2 py-1.5 text-[10px] text-ink-dim">
          Showing the first matches — keep typing to narrow.
        </div>
      )}
    </>
  );
}

function Heading({ children }: { children: React.ReactNode }) {
  return (
    <div className="px-2 pb-0.5 pt-1.5 text-[10px] font-semibold uppercase tracking-wider text-ink-dim">
      {children}
    </div>
  );
}

function LineList({
  path,
  lines,
  note,
  cursor,
  anchor,
  onPick,
}: {
  path: string;
  lines: string[] | null;
  note: string | null;
  cursor: number;
  anchor: number | null;
  onPick: (i: number) => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  useEffect(() => {
    ref.current?.scrollIntoView({ block: "nearest" });
  }, [cursor]);

  if (note) return <div className="px-2 py-3 text-xs text-ink-dim">{note}</div>;
  if (!lines) return <div className="px-2 py-3 text-xs text-ink-dim">Loading {path}…</div>;

  // Render a window around the cursor rather than the whole file.
  const from = Math.max(0, cursor - Math.floor(LINE_WINDOW / 2));
  const to = Math.min(lines.length, from + LINE_WINDOW);
  const lo = Math.min(anchor ?? cursor, cursor);
  const hi = Math.max(anchor ?? cursor, cursor);

  return (
    <>
      <div className="px-2 py-1 text-[10px] text-ink-dim">
        {path} · ↑↓ to move, Enter to insert, Shift+Enter for a range, Esc to go back
      </div>
      {lines.slice(from, to).map((line, k) => {
        const i = from + k;
        const selected = i >= lo && i <= hi;
        return (
          <div
            key={i}
            ref={i === cursor ? ref : undefined}
            onMouseDown={(e) => {
              e.preventDefault();
              onPick(i);
            }}
            className={`flex cursor-pointer gap-2 rounded px-2 font-mono text-xs leading-5 ${
              selected ? "bg-panel-2" : ""
            } ${i === cursor ? "outline outline-1 outline-accent" : ""}`}
          >
            <span className="w-10 shrink-0 select-none text-right text-ink-dim">{i + 1}</span>
            <span className="truncate">{line || " "}</span>
          </div>
        );
      })}
    </>
  );
}
