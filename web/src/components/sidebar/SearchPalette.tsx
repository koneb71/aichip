import { useEffect, useMemo, useRef, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { useNavigate } from "react-router-dom";
import { api, SearchHit, SearchResults } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";

const EMPTY: SearchResults = {
  projects: [],
  tasks: [],
  agents: [],
  teams: [],
  workflows: [],
};

/** A flattened hit plus where selecting it should take you. */
interface Row {
  hit: SearchHit;
  group: string;
  to: string;
}

/**
 * Workspace-wide search. Replaces the old box that only filtered the five
 * "Recent" projects already on screen.
 */
export function SearchPalette() {
  const { active } = useWorkspace();
  const navigate = useNavigate();
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResults>(EMPTY);
  const [open, setOpen] = useState(false);
  const [cursor, setCursor] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  // Cmd/Ctrl-K focuses search from anywhere.
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        inputRef.current?.focus();
        inputRef.current?.select();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  // Debounced fetch. `stale` guards against an earlier, slower response
  // landing after a later one and clobbering it.
  useEffect(() => {
    if (!active || query.trim().length < 2) {
      setResults(EMPTY);
      return;
    }
    let stale = false;
    const timer = setTimeout(() => {
      api
        .search(active.id, query.trim())
        .then((r) => {
          if (!stale) {
            setResults(r);
            setCursor(0);
          }
        })
        .catch(() => {
          if (!stale) setResults(EMPTY);
        });
    }, 180);
    return () => {
      stale = true;
      clearTimeout(timer);
    };
  }, [active, query]);

  const rows = useMemo<Row[]>(
    () => [
      ...results.projects.map((h) => ({ hit: h, group: "Projects", to: `/projects/${h.id}` })),
      ...results.tasks.map((h) => ({
        hit: h,
        group: "Tasks",
        to: `/projects/${h.projectId}`,
      })),
      ...results.workflows.map((h) => ({
        hit: h,
        group: "Workflows",
        to: `/projects/${h.projectId}`,
      })),
      ...results.agents.map((h) => ({ hit: h, group: "Agents", to: "/agents" })),
      ...results.teams.map((h) => ({ hit: h, group: "Teams", to: "/teams" })),
    ],
    [results],
  );

  const go = (row: Row) => {
    navigate(row.to);
    setOpen(false);
    setQuery("");
    inputRef.current?.blur();
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "Escape") {
      setOpen(false);
      inputRef.current?.blur();
      return;
    }
    if (!rows.length) return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setCursor((c) => (c + 1) % rows.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setCursor((c) => (c - 1 + rows.length) % rows.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[cursor];
      if (row) go(row);
    }
  };

  const showPanel = open && query.trim().length >= 2;

  return (
    <div className="relative">
      <input
        ref={inputRef}
        value={query}
        onChange={(e) => {
          setQuery(e.target.value);
          setOpen(true);
        }}
        onFocus={() => setOpen(true)}
        onKeyDown={onKeyDown}
        placeholder="Search…  ⌘K"
        className="mt-1 w-full rounded-lg border border-line bg-surface px-3 py-1.5 text-sm outline-none focus:border-accent"
      />

      <AnimatePresence>
        {showPanel && (
          <>
            <div className="fixed inset-0 z-10" onClick={() => setOpen(false)} />
            <motion.div
              initial={{ opacity: 0, y: -4 }}
              animate={{ opacity: 1, y: 0 }}
              exit={{ opacity: 0, y: -4 }}
              className="absolute left-0 right-0 top-full z-20 mt-1 max-h-96 overflow-y-auto rounded-xl border border-line bg-panel p-1 shadow-lg"
            >
              {rows.length === 0 ? (
                <div className="px-2 py-3 text-xs text-ink-dim">
                  Nothing matches “{query.trim()}”.
                </div>
              ) : (
                rows.map((row, i) => {
                  const firstOfGroup = i === 0 || rows[i - 1].group !== row.group;
                  return (
                    <div key={`${row.group}-${row.hit.id}`}>
                      {firstOfGroup && (
                        <div className="px-2 pb-0.5 pt-2 text-[10px] font-semibold uppercase tracking-wider text-ink-dim">
                          {row.group}
                        </div>
                      )}
                      <button
                        onMouseEnter={() => setCursor(i)}
                        onClick={() => go(row)}
                        className={`flex w-full flex-col rounded-lg px-2 py-1.5 text-left ${
                          i === cursor ? "bg-panel-2" : ""
                        }`}
                      >
                        <span className="truncate text-sm">{row.hit.label}</span>
                        {row.hit.sublabel && (
                          <span className="truncate text-[11px] text-ink-dim">
                            {row.hit.sublabel}
                          </span>
                        )}
                      </button>
                    </div>
                  );
                })
              )}
            </motion.div>
          </>
        )}
      </AnimatePresence>
    </div>
  );
}
