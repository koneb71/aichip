import { useEffect, useRef, useState } from "react";
import { api, FsListing } from "../lib/api";
import { Dialog } from "./ui/Dialog";
import { Button } from "./ui/Button";
import { Input } from "./ui/Field";
import { Badge } from "./ui/Badge";

/**
 * Browse for a folder.
 *
 * Two jobs, which are not the same job. Adding a project cares how the folder
 * ends up version controlled and says so before closing; *choosing a place to
 * put something* does not — the answer is a path. So `onPick` may resolve with
 * nothing, and when it does this closes without a word about git.
 */
export function FolderBrowserModal({
  onClose,
  onPick,
  title = "Choose a project folder",
  confirmLabel = "Use this folder",
  start,
  initialisesGit = true,
}: {
  onClose: () => void;
  /**
   * Resolves with how the project ended up being version controlled — or with
   * nothing, when the caller only wanted the path.
   */
  onPick: (path: string) => Promise<{ vcs: string; vcsNote: string | null } | void>;
  title?: string;
  confirmLabel?: string;
  /** Where to open. Omitted starts at the folder Eren browses from. */
  start?: string;
  /**
   * Whether picking this folder makes it a repository. A claim about what the
   * *caller* does, so the caller says it — a picker choosing where to clone
   * into initialises nothing.
   */
  initialisesGit?: boolean;
}) {
  const [listing, setListing] = useState<FsListing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Creating a folder, so a project can start from nothing rather than
  // requiring a trip to a terminal first.
  const [newName, setNewName] = useState<string | null>(null);
  // Set when a project was added but couldn't get a repository — worth a
  // beat of the user's attention rather than a silent close.
  const [noVcs, setNoVcs] = useState<string | null>(null);
  const newInput = useRef<HTMLInputElement>(null);
  // The dialog hears Escape before the input does (Radix listens on the
  // document, capturing), so which key closed it is noted on the way down.
  const escInForm = useRef(false);
  const naming = newName !== null;
  useEffect(() => {
    if (!naming) return;
    const onKey = (e: KeyboardEvent) => {
      escInForm.current = e.key === "Escape" && document.activeElement === newInput.current;
    };
    window.addEventListener("keydown", onKey, true);
    return () => window.removeEventListener("keydown", onKey, true);
  }, [naming]);

  const load = (path?: string) =>
    api
      .fsList(path)
      .then(setListing)
      .catch((e) => setError(String(e)));

  useEffect(() => {
    load(start);
    // Only on mount: re-running this on every `start` change would yank the
    // browser back while somebody is navigating.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // No separate "initialize git" step: adding the project does it server-side
  // when the folder needs it, because a repository is what buys the isolated
  // worktree and the reviewable diff.
  const useFolder = async () => {
    if (!listing || busy) return;
    setBusy(true);
    setError(null);
    try {
      const result = await onPick(listing.path);
      // Nothing to report means the caller only wanted a path.
      if (!result || result.vcs === "git") onClose();
      else setNoVcs(result.vcsNote ?? "This folder has no version control.");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  /** Create the folder and step into it — the point of making one is to use it. */
  const createFolder = async () => {
    if (!listing || !newName?.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      const created = await api.fsMkdir(listing.path, newName.trim());
      setNewName(null);
      await load(created.path);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const crumbs = listing?.path.split("/").filter(Boolean) ?? [];

  return (
    <Dialog
      open
      onOpenChange={(o) => {
        if (o) return;
        // Escape while naming a new folder closes the form, not the browser.
        // A click outside still closes the browser, as it always did.
        if (escInForm.current) {
          escInForm.current = false;
          setNewName(null);
        } else onClose();
      }}
      title={title}
      width={576}
      footer={
        // The notices sit above the footer row, outside the scrolling list, so
        // a long folder never pushes them out of sight.
        <div className="flex w-full min-w-0 flex-col gap-2">
          {error && (
            <div className="rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
              {error}
            </div>
          )}

          {noVcs && (
            <div className="rounded-lg border border-warning/40 bg-warning-subtle px-3 py-2 text-xs text-warning-fg">
              <div className="font-medium">Added without version control</div>
              <div className="mt-0.5">{noVcs}</div>
              <div className="mt-1">
                Tasks here edit the folder directly — no isolated worktree, no diff to
                review, and no undo.
              </div>
            </div>
          )}

          <div className="flex items-center gap-2">
            <div className="min-w-0 flex-1 truncate text-xs text-fg-muted">
              {listing?.path}
              {listing && !listing.isGitRepo && !noVcs && initialisesGit && (
                <span className="ml-1 opacity-80">· git will be initialized here</span>
              )}
            </div>
            {noVcs ? (
              <Button variant="primary" onClick={onClose}>
                Got it
              </Button>
            ) : (
              <Button variant="primary" onClick={useFolder} disabled={busy || !listing}>
                {busy ? "Working…" : confirmLabel}
              </Button>
            )}
          </div>
        </div>
      }
    >
      <div className="sticky top-0 z-10 -mx-5 -mt-4 border-b border-border bg-raised px-5 py-3">
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 flex-wrap items-center gap-1 text-xs text-fg-muted">
            <button onClick={() => load()} className="hover:text-accent-fg">
              ~
            </button>
            {crumbs.map((c, i) => (
              <span key={i} className="flex items-center gap-1">
                <span>/</span>
                <button
                  onClick={() => load("/" + crumbs.slice(0, i + 1).join("/"))}
                  className="hover:text-accent-fg"
                >
                  {c}
                </button>
              </span>
            ))}
          </div>
          <Button size="sm" onClick={() => setNewName(newName === null ? "" : null)}>
            + New folder
          </Button>
        </div>
      </div>

      {newName !== null && (
        <form
          onSubmit={(e) => {
            e.preventDefault();
            createFolder();
          }}
          className="-mx-5 flex items-center gap-2 border-b border-border px-5 py-2.5"
        >
          <span className="text-xs text-fg-muted">New folder in {listing?.path ?? "…"}</span>
          <Input
            autoFocus
            value={newName}
            onChange={(e) => setNewName(e.target.value)}
            ref={newInput}
            placeholder="my-project"
            className="min-w-0 flex-1"
          />
          <Button type="submit" variant="primary" size="sm" disabled={busy || !newName.trim()}>
            Create
          </Button>
        </form>
      )}

      <div className="-mx-3 py-2">
        {listing?.parent && (
          <button
            onClick={() => load(listing.parent!)}
            className="w-full rounded-lg px-3 py-1.5 text-left text-sm text-fg-muted hover:bg-panel-2"
          >
            ← ..
          </button>
        )}
        {listing?.dirs.map((d) => (
          <button
            key={d.path}
            onClick={() => load(d.path)}
            className="flex w-full items-center gap-2 rounded-lg px-3 py-1.5 text-left text-sm hover:bg-panel-2"
          >
            <span className="text-fg-muted">▸</span>
            <span className="min-w-0 flex-1 truncate">{d.name}</span>
            {d.isGitRepo && <Badge tone="easy">git</Badge>}
          </button>
        ))}
        {listing && listing.dirs.length === 0 && (
          <div className="p-3 text-sm text-fg-muted">
            No subfolders here — use this folder, or create one.
          </div>
        )}
      </div>
    </Dialog>
  );
}
