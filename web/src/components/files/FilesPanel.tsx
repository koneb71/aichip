import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  CheckoutState,
  FileConflictError,
  FileEntry,
  Task,
  Tree,
} from "../../lib/api";
import { languageFor } from "../../lib/language";
import { NARROW, useMediaQuery } from "../../lib/useMediaQuery";

/**
 * Monaco arrives only when someone opens a file.
 *
 * `FilesPanel` is a tab and therefore always in the main chunk, so the lazy
 * boundary has to sit here rather than around the panel. Same shape as
 * `main.tsx`'s `PageEditor` and `PageBody`'s highlight.js import.
 */
const CodeEditor = lazy(() => import("../editor/CodeEditor"));

import { SourceControlBar } from "./SourceControlBar";

/**
 * The IDE tab: explorer tree, editor tabs, Monaco, a status bar — laid out
 * and painted the way the editor it embeds is, which is why this panel is a
 * dark island in a light app. The palette is VS Code's, hardcoded rather than
 * themed: index.css has one light theme, and these colours belong to this
 * shell, not to the app.
 *
 * Editing a worktree edits the change you are about to review and merge —
 * that is the point of offering it, and the reason the tree you are in is
 * named at the top rather than left to be inferred.
 */

/** One open file. `draft === null` means untouched since load. */
interface Buffer {
  content: string | null;
  draft: string | null;
  hash: string | null;
  size: number;
  readOnly: string | null;
  tooLarge: boolean;
  binary: boolean;
}

const isDirty = (b: Buffer | undefined) =>
  !!b && b.draft !== null && b.content !== null && b.draft !== b.content;

export function FilesPanel({
  projectId,
  tasks = [],
}: {
  projectId: string;
  /** For the tree selector: any card with a branch has a worktree to browse. */
  tasks?: Task[];
}) {
  const narrow = useMediaQuery(NARROW);
  const [tree, setTree] = useState<Tree>({ kind: "project", id: projectId });
  const [error, setError] = useState<string | null>(null);

  // The explorer: entries per directory, fetched on first expand.
  const [dirs, setDirs] = useState<Map<string, FileEntry[]>>(new Map());
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // The editor: one buffer per open file, tabs in opening order.
  const [buffers, setBuffers] = useState<Map<string, Buffer>>(new Map());
  const [openTabs, setOpenTabs] = useState<string[]>([]);
  const [active, setActive] = useState<string | null>(null);
  const [loadingFile, setLoadingFile] = useState(false);

  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{ hash: string; content: string } | null>(null);
  // Bumped on save so the source-control bar re-reads the checkout status.
  const [scmKey, setScmKey] = useState(0);
  // Lifted from the source-control bar for the status bar's branch segment.
  const [checkout, setCheckout] = useState<CheckoutState | null>(null);
  const [cursor, setCursor] = useState<{ line: number; col: number } | null>(null);

  const treeKey = `${tree.kind}:${tree.id}`;
  const buffer = active ? buffers.get(active) : undefined;
  const dirty = isDirty(buffer);
  const anyDirty = [...buffers.values()].some((b) => isDirty(b));

  const openTask =
    tree.kind === "task" ? tasks.find((t) => t.id === tree.id) ?? null : null;
  // A run in this tree can rewrite the whole file from its own context, with no
  // conflict for us to catch — see the banner text.
  const agentBusy =
    tree.kind === "task" &&
    !!openTask &&
    (openTask.runStatus === "running" ||
      openTask.runStatus === "starting" ||
      openTask.runStatus === "queued" ||
      openTask.runStatus === "waiting_permission");

  const loadDir = useCallback(
    (path: string) => {
      api
        .files(tree, path)
        .then((l) => {
          setDirs((prev) => new Map(prev).set(path, l.entries));
        })
        .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
    },
    [tree],
  );

  // Reset when the tree changes, or we'd browse the old one's paths.
  useEffect(() => {
    setDirs(new Map());
    setExpanded(new Set());
    setBuffers(new Map());
    setOpenTabs([]);
    setActive(null);
    setConflict(null);
    setSaveError(null);
    setError(null);
    setCursor(null);
    loadDir("");
  }, [treeKey, loadDir]);

  // Follow the project prop, so switching projects does not leave us pointed at
  // the previous one's checkout.
  useEffect(() => {
    setTree({ kind: "project", id: projectId });
  }, [projectId]);

  /** Nothing typed is thrown away without being asked first. */
  const mayLeave = useCallback(() => {
    if (!anyDirty) return true;
    return window.confirm("You have unsaved changes here. Discard them?");
  }, [anyDirty]);

  // The panel unmounts when you switch tabs, so this is the only thing between
  // a half-typed edit and a closed window.
  useEffect(() => {
    if (!anyDirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [anyDirty]);

  const toggleDir = (path: string) => {
    setExpanded((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
    if (!dirs.has(path)) loadDir(path);
  };

  const openFile = useCallback(
    (path: string) => {
      setConflict(null);
      setSaveError(null);
      if (buffers.has(path)) {
        setActive(path);
        return;
      }
      setLoadingFile(true);
      setActive(path);
      setOpenTabs((tabs) => (tabs.includes(path) ? tabs : [...tabs, path]));
      api
        .file(tree, path)
        .then((f) => {
          setBuffers((prev) =>
            new Map(prev).set(path, {
              content: f.content,
              draft: null,
              hash: f.hash,
              size: f.size,
              readOnly: f.readOnly ?? null,
              tooLarge: !!f.tooLarge,
              binary: !!f.binary,
            }),
          );
        })
        .catch((e) => setError(String(e).replace(/^Error:\s*/, "")))
        .finally(() => setLoadingFile(false));
    },
    [tree, buffers],
  );

  const closeTab = (path: string) => {
    const b = buffers.get(path);
    if (isDirty(b) && !window.confirm(`${basename(path)} has unsaved changes. Discard them?`)) {
      return;
    }
    setOpenTabs((tabs) => {
      const next = tabs.filter((t) => t !== path);
      if (active === path) setActive(next[next.length - 1] ?? null);
      return next;
    });
    setBuffers((prev) => {
      const next = new Map(prev);
      next.delete(path);
      return next;
    });
  };

  const setDraft = (path: string, draft: string) => {
    setBuffers((prev) => {
      const b = prev.get(path);
      if (!b) return prev;
      return new Map(prev).set(path, { ...b, draft });
    });
  };

  // Held in a ref so the editor's ⌘S command, registered once, always calls the
  // current one rather than the closure it was created with.
  const saveRef = useRef<() => void>(() => {});
  const save = useCallback(async () => {
    const path = active;
    const b = path ? buffers.get(path) : undefined;
    if (!path || !b || b.draft === null || saving) return;
    setSaving(true);
    setSaveError(null);
    setConflict(null);
    try {
      const r = await api.saveFile(tree, path, b.draft, b.hash);
      // The saved text is now what is on disk, which is what clears `dirty`.
      setBuffers((prev) => {
        const cur = prev.get(path);
        if (!cur) return prev;
        return new Map(prev).set(path, {
          ...cur,
          content: cur.draft,
          hash: r.hash,
          size: r.size,
        });
      });
      // And what makes the checkout dirty — the source-control bar re-reads.
      setScmKey((k) => k + 1);
    } catch (e) {
      if (e instanceof FileConflictError) {
        setConflict({
          hash: e.conflict.currentHash,
          content: e.conflict.currentContent,
        });
      } else {
        setSaveError(String(e).replace(/^Error:\s*/, ""));
      }
    } finally {
      setSaving(false);
    }
  }, [tree, active, buffers, saving]);
  saveRef.current = save;

  // Below `lg` the tree and the editor take turns: two 50%-wide panes would
  // make both the paths and the code unreadable.
  const showTree = !narrow || !active;
  const showViewer = !narrow || !!active;

  const editable = !!buffer && buffer.hash !== null && !buffer.readOnly;

  return (
    <div className="flex h-full min-h-0 flex-col bg-[#1e1e1e] text-[#cccccc]">
      <div className="grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)] lg:grid-cols-[280px_minmax(0,1fr)]">
        {/* ── Explorer ─────────────────────────────────────────────────── */}
        <div
          className={`${showTree ? "flex" : "hidden"} min-h-0 min-w-0 flex-col bg-[#252526] lg:flex lg:border-r lg:border-[#3c3c3c]`}
        >
          <div className="px-3 pb-1 pt-2.5 text-[10px] font-semibold uppercase tracking-widest text-[#8c8c8c]">
            Explorer
          </div>
          <TreePicker
            tree={tree}
            projectId={projectId}
            tasks={tasks}
            onPick={(next) => {
              if (mayLeave()) setTree(next);
            }}
          />
          {/* Only for the checkout: a worktree already has its own lifecycle —
              review, merge, PR — and offering push there would route around it. */}
          {tree.kind === "project" && (
            <SourceControlBar
              projectId={projectId}
              refreshKey={scmKey}
              onState={setCheckout}
            />
          )}
          <div className="min-h-0 flex-1 overflow-y-auto py-1">
            <DirEntries
              dir=""
              depth={0}
              dirs={dirs}
              expanded={expanded}
              active={active}
              openTabs={openTabs}
              buffers={buffers}
              onToggle={toggleDir}
              onOpen={openFile}
            />
          </div>
        </div>

        {/* ── Editor column ────────────────────────────────────────────── */}
        <div className={`${showViewer ? "flex" : "hidden"} min-h-0 min-w-0 flex-col lg:flex`}>
          {/* Tabs, VS Code's shape: active tab merges into the editor. */}
          {openTabs.length > 0 && (
            <div className="flex items-stretch overflow-x-auto bg-[#252526]">
              {narrow && (
                <button
                  onClick={() => setActive(null)}
                  className="shrink-0 px-2 text-xs text-[#8c8c8c] hover:text-white"
                >
                  ←
                </button>
              )}
              {openTabs.map((path) => {
                const b = buffers.get(path);
                const isActive = path === active;
                return (
                  <div
                    key={path}
                    className={`group flex shrink-0 cursor-pointer items-center gap-1.5 border-r border-[#3c3c3c] px-3 py-1.5 text-xs ${
                      isActive
                        ? "border-t border-t-[#0e639c] bg-[#1e1e1e] text-white"
                        : "bg-[#2d2d2d] text-[#969696] hover:text-white"
                    }`}
                    onClick={() => {
                      setConflict(null);
                      setSaveError(null);
                      setActive(path);
                    }}
                    title={path}
                  >
                    <span>{basename(path)}</span>
                    <button
                      onClick={(e) => {
                        e.stopPropagation();
                        closeTab(path);
                      }}
                      title={isDirty(b) ? "Unsaved changes" : "Close"}
                      className="w-3.5 text-center leading-none"
                    >
                      {isDirty(b) ? (
                        <span className="text-white">●</span>
                      ) : (
                        <span className="opacity-0 hover:!opacity-100 group-hover:opacity-60">
                          ×
                        </span>
                      )}
                    </button>
                  </div>
                );
              })}
              {editable && (
                <button
                  onClick={save}
                  disabled={!dirty || saving}
                  className="ml-auto mr-2 shrink-0 self-center rounded bg-[#0e639c] px-2.5 py-0.5 text-xs text-white hover:bg-[#1177bb] disabled:opacity-40"
                  title="⌘S"
                >
                  {saving ? "Saving…" : "Save"}
                </button>
              )}
            </div>
          )}

          {error && (
            <div className="border-b border-[#5a1d1d] bg-[#5a1d1d]/40 px-4 py-1.5 text-[11px] text-[#f48771]">
              {error}
            </div>
          )}
          {!active && !error && (
            <div className="mt-16 px-6 text-center text-sm text-[#8c8c8c]">
              Select a file to open it. Edits here are yours —{" "}
              <span className="text-[#cccccc]">agents still work only in worktrees</span>,
              which is what keeps a run reviewable.
            </div>
          )}
          {active && (
            <>
              {buffer?.readOnly && <Note tone="amber">{buffer.readOnly}</Note>}
              {agentBusy && (
                // Stated precisely, because the obvious reading is wrong. The
                // hash check stops *you* landing on the agent's bytes; nothing
                // stops the agent rewriting this file whole from its own context
                // and taking your save with it, silently and without a conflict.
                <Note tone="amber">
                  An agent is working in this tree — it may overwrite what you save.
                </Note>
              )}
              {conflict && (
                <div className="border-b border-[#6b5900] bg-[#3a3100] px-4 py-2 text-xs text-[#e2c08d]">
                  <span className="font-semibold">
                    This file changed on disk since you opened it.
                  </span>
                  <div className="mt-1.5 flex flex-wrap gap-2">
                    <button
                      onClick={() => {
                        const path = active;
                        setBuffers((prev) => {
                          const cur = prev.get(path);
                          if (!cur) return prev;
                          return new Map(prev).set(path, {
                            ...cur,
                            content: conflict.content,
                            hash: conflict.hash,
                            draft: null,
                          });
                        });
                        setConflict(null);
                      }}
                      className="rounded border border-[#6b5900] bg-[#1e1e1e] px-2 py-1 font-medium hover:bg-[#2a2d2e]"
                    >
                      Load theirs
                    </button>
                    <button
                      onClick={() => {
                        // Re-save against the *current* hash, so the
                        // compare-and-swap still happens. An escape hatch that
                        // skipped the check would be the check not existing.
                        const path = active;
                        setBuffers((prev) => {
                          const cur = prev.get(path);
                          if (!cur) return prev;
                          return new Map(prev).set(path, { ...cur, hash: conflict.hash });
                        });
                        setConflict(null);
                        setTimeout(() => saveRef.current(), 0);
                      }}
                      className="rounded border border-[#6b5900] bg-[#1e1e1e] px-2 py-1 hover:bg-[#2a2d2e]"
                    >
                      Keep mine
                    </button>
                  </div>
                </div>
              )}
              {saveError && <Note tone="red">{saveError}</Note>}

              <div className="min-h-0 flex-1">
                {loadingFile && !buffer && (
                  <div className="p-4 text-xs text-[#8c8c8c]">Loading…</div>
                )}
                {buffer?.tooLarge && (
                  <div className="p-4 text-sm text-[#8c8c8c]">
                    This file is {humanSize(buffer.size)} — too large to open.
                  </div>
                )}
                {buffer?.binary && (
                  <div className="p-4 text-sm text-[#8c8c8c]">
                    Binary file ({humanSize(buffer.size)}), not shown.
                  </div>
                )}
                {buffer && buffer.content !== null && (
                  <Suspense
                    fallback={<div className="p-4 text-xs text-[#8c8c8c]">Loading editor…</div>}
                  >
                    <CodeEditor
                      path={`${treeKey}/${active}`}
                      language={languageFor(active)}
                      value={buffer.draft ?? buffer.content}
                      readOnly={!editable}
                      dark
                      onChange={(next) => setDraft(active, next)}
                      onSave={() => saveRef.current()}
                      onCursor={(line, col) => setCursor({ line, col })}
                    />
                  </Suspense>
                )}
              </div>
            </>
          )}
        </div>
      </div>

      {/* ── Status bar — the one VS Code strip everyone recognises. ────── */}
      <div className="flex items-center gap-3 bg-[#007acc] px-3 py-0.5 text-[11px] text-white">
        {tree.kind === "project" && checkout?.branch && (
          <span className="flex items-center gap-1" title="Current branch">
            ⎇ {checkout.branch}
            {(checkout.behind ?? 0) > 0 && ` ↓${checkout.behind}`}
            {(checkout.ahead ?? 0) > 0 && ` ↑${checkout.ahead}`}
          </span>
        )}
        {tree.kind === "task" && <span>worktree: {openTask?.title ?? "card"}</span>}
        {active && (
          <span className="min-w-0 truncate opacity-90" title={active}>
            {active}
            {dirty && " ●"}
          </span>
        )}
        <span className="ml-auto flex shrink-0 items-center gap-3 opacity-90">
          {buffer && !buffer.tooLarge && !buffer.binary && <span>{humanSize(buffer.size)}</span>}
          {cursor && active && (
            <span>
              Ln {cursor.line}, Col {cursor.col}
            </span>
          )}
          {active && <span>{languageFor(active)}</span>}
          {buffer?.readOnly && <span>read-only</span>}
          <span>UTF-8</span>
        </span>
      </div>
    </div>
  );
}

/** Recursive explorer entries: folders expand in place, VS Code style. */
function DirEntries({
  dir,
  depth,
  dirs,
  expanded,
  active,
  openTabs,
  buffers,
  onToggle,
  onOpen,
}: {
  dir: string;
  depth: number;
  dirs: Map<string, FileEntry[]>;
  expanded: Set<string>;
  active: string | null;
  openTabs: string[];
  buffers: Map<string, Buffer>;
  onToggle: (path: string) => void;
  onOpen: (path: string) => void;
}) {
  const entries = dirs.get(dir);
  if (!entries) {
    return (
      <div style={{ paddingLeft: depth * 12 + 22 }} className="py-1 text-[11px] text-[#8c8c8c]">
        …
      </div>
    );
  }
  if (entries.length === 0 && dir === "") {
    return <div className="px-3 py-2 text-xs text-[#8c8c8c]">Empty folder.</div>;
  }
  return (
    <>
      {entries.map((entry) => {
        const pad = depth * 12 + 8;
        if (entry.kind === "dir") {
          const open = expanded.has(entry.path);
          return (
            <div key={entry.path}>
              <button
                onClick={() => onToggle(entry.path)}
                style={{ paddingLeft: pad }}
                className="flex w-full items-center gap-1 py-[3px] pr-2 text-left text-[13px] text-[#cccccc] hover:bg-[#2a2d2e]"
              >
                <span className="w-3 text-center text-[10px] text-[#8c8c8c]">
                  {open ? "▾" : "▸"}
                </span>
                <span className="truncate">{entry.name}</span>
              </button>
              {open && (
                <DirEntries
                  dir={entry.path}
                  depth={depth + 1}
                  dirs={dirs}
                  expanded={expanded}
                  active={active}
                  openTabs={openTabs}
                  buffers={buffers}
                  onToggle={onToggle}
                  onOpen={onOpen}
                />
              )}
            </div>
          );
        }
        const isActive = active === entry.path;
        const isOpen = openTabs.includes(entry.path);
        const dirtyDot = isDirty(buffers.get(entry.path));
        return (
          <button
            key={entry.path}
            onClick={() => onOpen(entry.path)}
            style={{ paddingLeft: pad + 16 }}
            className={`flex w-full items-center gap-1.5 py-[3px] pr-2 text-left text-[13px] ${
              isActive
                ? "bg-[#37373d] text-white"
                : isOpen
                  ? "text-[#e7e7e7] hover:bg-[#2a2d2e]"
                  : "text-[#a9a9a9] hover:bg-[#2a2d2e] hover:text-[#cccccc]"
            }`}
            title={entry.path}
          >
            <span className="truncate">{entry.name}</span>
            {dirtyDot && <span className="text-[9px] text-white">●</span>}
            {entry.size !== null && !dirtyDot && (
              <span className="ml-auto shrink-0 text-[10px] text-[#6e7681]">
                {humanSize(entry.size)}
              </span>
            )}
          </button>
        );
      })}
    </>
  );
}

function Note({ tone, children }: { tone: "amber" | "red"; children: React.ReactNode }) {
  const cls =
    tone === "amber"
      ? "border-[#6b5900] bg-[#3a3100] text-[#e2c08d]"
      : "border-[#5a1d1d] bg-[#5a1d1d]/40 text-[#f48771]";
  return <div className={`border-b px-4 py-1.5 text-[11px] ${cls}`}>{children}</div>;
}

/**
 * Which tree you are looking at.
 *
 * Only cards with a branch are offered — that is the signal a worktree exists,
 * and it avoids putting an absolute host path in the API purely to populate a
 * dropdown.
 */
function TreePicker({
  tree,
  projectId,
  tasks,
  onPick,
}: {
  tree: Tree;
  projectId: string;
  tasks: Task[];
  onPick: (t: Tree) => void;
}) {
  const withBranches = tasks.filter((t) => t.branch);
  if (withBranches.length === 0) return null;

  const value = tree.kind === "project" ? "" : tree.id;
  return (
    <div className="border-b border-[#3c3c3c] px-3 py-1.5">
      <select
        value={value}
        onChange={(e) =>
          onPick(
            e.target.value
              ? { kind: "task", id: e.target.value }
              : { kind: "project", id: projectId },
          )
        }
        className="w-full rounded border border-[#3c3c3c] bg-[#3c3c3c] px-2 py-1 text-xs text-[#cccccc]"
      >
        <option value="">Checkout</option>
        {withBranches.map((t) => (
          <option key={t.id} value={t.id}>
            {t.title}
          </option>
        ))}
      </select>
    </div>
  );
}

function basename(path: string): string {
  return path.split("/").pop() ?? path;
}

function humanSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}
