import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, FsListing } from "../lib/api";

export function FolderBrowserModal({
  onClose,
  onPick,
}: {
  onClose: () => void;
  /** Resolves with how the project ended up being version controlled. */
  onPick: (path: string) => Promise<{ vcs: string; vcsNote: string | null }>;
}) {
  const [listing, setListing] = useState<FsListing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Set when a project was added but couldn't get a repository — worth a
  // beat of the user's attention rather than a silent close.
  const [noVcs, setNoVcs] = useState<string | null>(null);

  const load = (path?: string) =>
    api
      .fsList(path)
      .then(setListing)
      .catch((e) => setError(String(e)));

  useEffect(() => {
    load();
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
      if (result.vcs === "git") onClose();
      else setNoVcs(result.vcsNote ?? "This folder has no version control.");
    } catch (e) {
      setError(String(e));
    } finally {
      setBusy(false);
    }
  };

  const crumbs = listing?.path.split("/").filter(Boolean) ?? [];

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex max-h-[70vh] w-full max-w-xl flex-col rounded-2xl border border-line bg-panel"
      >
        <div className="border-b border-line p-4">
          <div className="text-base font-semibold">Load a project folder</div>
          <div className="mt-2 flex flex-wrap items-center gap-1 text-xs text-ink-dim">
            <button onClick={() => load()} className="hover:text-accent">
              ~
            </button>
            {crumbs.map((c, i) => (
              <span key={i} className="flex items-center gap-1">
                <span>/</span>
                <button
                  onClick={() => load("/" + crumbs.slice(0, i + 1).join("/"))}
                  className="hover:text-accent"
                >
                  {c}
                </button>
              </span>
            ))}
          </div>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto p-2">
          {listing?.parent && (
            <button
              onClick={() => load(listing.parent!)}
              className="w-full rounded-lg px-3 py-1.5 text-left text-sm text-ink-dim hover:bg-panel-2"
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
              <span className="text-ink-dim">▸</span>
              <span className="min-w-0 flex-1 truncate">{d.name}</span>
              {d.isGitRepo && (
                <span className="rounded-full bg-tier-easy-soft px-2 py-0.5 text-[11px] text-tier-easy">
                  git
                </span>
              )}
            </button>
          ))}
          {listing && listing.dirs.length === 0 && (
            <div className="p-3 text-sm text-ink-dim">No subfolders here.</div>
          )}
        </div>

        {error && (
          <div className="mx-4 mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        {noVcs && (
          <div className="mx-4 mb-2 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-700">
            <div className="font-medium">Added without version control</div>
            <div className="mt-0.5">{noVcs}</div>
            <div className="mt-1">
              Tasks here edit the folder directly — no isolated worktree, no diff to
              review, and no undo.
            </div>
          </div>
        )}

        <div className="flex items-center justify-between border-t border-line p-4">
          <div className="min-w-0 flex-1 truncate text-xs text-ink-dim">
            {listing?.path}
            {listing && !listing.isGitRepo && !noVcs && (
              <span className="ml-1 opacity-80">· git will be initialized here</span>
            )}
          </div>
          {noVcs ? (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={onClose}
              className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white"
            >
              Got it
            </motion.button>
          ) : (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={useFolder}
              disabled={busy || !listing}
              className="shrink-0 rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white disabled:opacity-50"
            >
              {busy ? "Adding…" : "Use this folder"}
            </motion.button>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
