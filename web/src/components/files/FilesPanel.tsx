import { lazy, Suspense, useCallback, useEffect, useRef, useState } from "react";
import {
  api,
  FileConflictError,
  FileContent,
  FileEntry,
  FileListing,
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
 * Browse and edit a project checkout, or a card's worktree.
 *
 * Editing a worktree edits the change you are about to review and merge —
 * that is the point of offering it, and the reason the tree you are in is
 * named at the top rather than left to be inferred.
 */
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
  const [listing, setListing] = useState<FileListing | null>(null);
  const [dir, setDir] = useState("");
  const [selected, setSelected] = useState<string | null>(null);
  const [file, setFile] = useState<FileContent | null>(null);
  const [loadingFile, setLoadingFile] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The editor's buffer. `null` means the file has not been touched.
  const [draft, setDraft] = useState<string | null>(null);
  const [baseHash, setBaseHash] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);
  const [conflict, setConflict] = useState<{ hash: string; content: string } | null>(null);
  // Bumped on save so the source-control bar re-reads the checkout status.
  const [scmKey, setScmKey] = useState(0);

  // Derived, never stored. Two sources of truth for "is this dirty" is how you
  // get a Save button that lies about whether there is anything to save.
  const dirty = draft !== null && file !== null && draft !== file.content;

  const treeKey = `${tree.kind}:${tree.id}`;
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

  // Reset when the tree changes, or we'd browse the old one's paths.
  useEffect(() => {
    setDir("");
    setSelected(null);
    setFile(null);
    setDraft(null);
    setBaseHash(null);
    setConflict(null);
    setSaveError(null);
  }, [treeKey]);

  // Follow the project prop, so switching projects does not leave us pointed at
  // the previous one's checkout.
  useEffect(() => {
    setTree({ kind: "project", id: projectId });
  }, [projectId]);

  useEffect(() => {
    let stale = false;
    setError(null);
    api
      .files(tree, dir)
      .then((l) => {
        if (!stale) setListing(l);
      })
      .catch((e) => {
        if (!stale) setError(String(e).replace(/^Error:\s*/, ""));
      });
    return () => {
      stale = true;
    };
  }, [treeKey, dir, tree]);

  /** Nothing typed is thrown away without being asked first. */
  const mayLeave = useCallback(() => {
    if (!dirty) return true;
    return window.confirm("You have unsaved changes here. Discard them?");
  }, [dirty]);

  // The panel unmounts when you switch tabs, so this is the only thing between
  // a half-typed edit and a closed window.
  useEffect(() => {
    if (!dirty) return;
    const warn = (e: BeforeUnloadEvent) => e.preventDefault();
    window.addEventListener("beforeunload", warn);
    return () => window.removeEventListener("beforeunload", warn);
  }, [dirty]);

  const load = useCallback(
    (path: string) => {
      setSelected(path);
      setLoadingFile(true);
      setFile(null);
      setDraft(null);
      setConflict(null);
      setSaveError(null);
      api
        .file(tree, path)
        .then((f) => {
          setFile(f);
          setBaseHash(f.hash);
        })
        .catch((e) => setError(String(e).replace(/^Error:\s*/, "")))
        .finally(() => setLoadingFile(false));
    },
    [tree],
  );

  const open = useCallback(
    (entry: FileEntry) => {
      if (!mayLeave()) return;
      if (entry.kind === "dir") {
        setDir(entry.path);
        return;
      }
      load(entry.path);
    },
    [load, mayLeave],
  );

  // Held in a ref so the editor's ⌘S command, registered once, always calls the
  // current one rather than the closure it was created with.
  const saveRef = useRef<() => void>(() => {});
  const save = useCallback(async () => {
    if (!selected || draft === null || saving) return;
    setSaving(true);
    setSaveError(null);
    setConflict(null);
    try {
      const r = await api.saveFile(tree, selected, draft, baseHash);
      setBaseHash(r.hash);
      // The saved text is now what is on disk, which is what clears `dirty`.
      setFile((f) => (f ? { ...f, content: draft, size: r.size, hash: r.hash } : f));
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
  }, [tree, selected, draft, baseHash, saving]);
  saveRef.current = save;

  // Below `lg` the tree and the editor take turns: two 50%-wide panes would
  // make both the paths and the code unreadable.
  const showTree = !narrow || !selected;
  const showViewer = !narrow || !!selected;

  const editable = !!file && file.hash !== null && !file.readOnly;

  return (
    <div className="grid h-full min-h-0 grid-cols-[minmax(0,1fr)] lg:grid-cols-[280px_minmax(0,1fr)]">
      <div
        className={`${showTree ? "flex" : "hidden"} min-h-0 min-w-0 flex-col border-line bg-panel lg:flex lg:border-r`}
      >
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
          <SourceControlBar projectId={projectId} refreshKey={scmKey} />
        )}
        <Breadcrumbs
          dir={dir}
          onNavigate={(p) => {
            if (mayLeave()) setDir(p);
          }}
        />
        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {listing?.parent !== null && listing !== null && (
            <button
              onClick={() => mayLeave() && setDir(listing.parent ?? "")}
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
            Select a file to open it. Edits here are yours —{" "}
            <span className="text-ink">agents still work only in worktrees</span>,
            which is what keeps a run reviewable.
          </div>
        )}
        {selected && (
          <>
            <div className="flex items-center gap-2 border-b border-line bg-panel px-4 py-2">
              {narrow && (
                <button
                  onClick={() => mayLeave() && setSelected(null)}
                  className="shrink-0 rounded-md px-1.5 py-0.5 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
                >
                  ← Files
                </button>
              )}
              <div className="truncate font-mono text-xs text-ink-dim">
                {selected}
                {dirty && <span className="ml-1 text-accent">•</span>}
              </div>
              {file && !file.tooLarge && (
                <span className="ml-auto shrink-0 text-[10px] text-ink-dim/70">
                  {humanSize(file.size)}
                </span>
              )}
              {editable && (
                <button
                  onClick={save}
                  disabled={!dirty || saving}
                  className="shrink-0 rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white disabled:opacity-40"
                >
                  {saving ? "Saving…" : "Save"}
                </button>
              )}
            </div>

            {file?.readOnly && (
              <Note tone="amber">{file.readOnly}</Note>
            )}
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
              <div className="border-b border-amber-300 bg-amber-50 px-4 py-2 text-xs text-amber-900">
                <span className="font-semibold">
                  This file changed on disk since you opened it.
                </span>
                <div className="mt-1.5 flex flex-wrap gap-2">
                  <button
                    onClick={() => {
                      setFile((f) =>
                        f ? { ...f, content: conflict.content, hash: conflict.hash } : f,
                      );
                      setBaseHash(conflict.hash);
                      setDraft(null);
                      setConflict(null);
                    }}
                    className="rounded-lg border border-amber-400 bg-panel px-2 py-1 font-medium hover:bg-amber-100"
                  >
                    Load theirs
                  </button>
                  <button
                    onClick={() => {
                      // Re-save against the *current* hash, so the
                      // compare-and-swap still happens. An escape hatch that
                      // skipped the check would be the check not existing.
                      setBaseHash(conflict.hash);
                      setConflict(null);
                      setTimeout(() => saveRef.current(), 0);
                    }}
                    className="rounded-lg border border-amber-400 bg-panel px-2 py-1 hover:bg-amber-100"
                  >
                    Keep mine
                  </button>
                </div>
              </div>
            )}
            {saveError && <Note tone="red">{saveError}</Note>}

            <div className="min-h-0 flex-1">
              {loadingFile && <div className="p-4 text-xs text-ink-dim">Loading…</div>}
              {file?.tooLarge && (
                <div className="p-4 text-sm text-ink-dim">
                  This file is {humanSize(file.size)} — too large to open.
                </div>
              )}
              {file?.binary && (
                <div className="p-4 text-sm text-ink-dim">
                  Binary file ({humanSize(file.size)}), not shown.
                </div>
              )}
              {file && file.content !== null && (
                <Suspense
                  fallback={<div className="p-4 text-xs text-ink-dim">Loading editor…</div>}
                >
                  <CodeEditor
                    path={`${treeKey}/${selected}`}
                    language={languageFor(selected)}
                    value={draft ?? file.content}
                    readOnly={!editable}
                    onChange={setDraft}
                    onSave={() => saveRef.current()}
                  />
                </Suspense>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

function Note({ tone, children }: { tone: "amber" | "red"; children: React.ReactNode }) {
  const cls =
    tone === "amber"
      ? "border-amber-300 bg-amber-50 text-amber-900"
      : "border-red-200 bg-red-50 text-danger";
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
    <div className="border-b border-line px-3 py-2">
      <select
        value={value}
        onChange={(e) =>
          onPick(
            e.target.value
              ? { kind: "task", id: e.target.value }
              : { kind: "project", id: projectId },
          )
        }
        className="w-full rounded-lg border border-line bg-panel px-2 py-1 text-xs"
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
