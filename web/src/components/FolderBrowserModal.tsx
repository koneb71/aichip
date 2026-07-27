import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, FsListing } from "../lib/api";

export function FolderBrowserModal({
  onClose,
  onPick,
}: {
  onClose: () => void;
  onPick: (path: string) => Promise<void>;
}) {
  const [listing, setListing] = useState<FsListing | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const load = (path?: string) =>
    api
      .fsList(path)
      .then(setListing)
      .catch((e) => setError(String(e)));

  useEffect(() => {
    load();
  }, []);

  const useFolder = async (initFirst: boolean) => {
    if (!listing || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (initFirst) await api.gitInit(listing.path);
      await onPick(listing.path);
      onClose();
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

        <div className="flex items-center justify-between border-t border-line p-4">
          <div className="min-w-0 truncate text-xs text-ink-dim">{listing?.path}</div>
          {listing?.isGitRepo ? (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={() => useFolder(false)}
              disabled={busy}
              className="rounded-lg bg-accent px-4 py-1.5 text-sm font-medium text-white"
            >
              {busy ? "Adding…" : "Use this folder"}
            </motion.button>
          ) : (
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={() => useFolder(true)}
              disabled={busy || !listing}
              className="rounded-lg border border-accent px-4 py-1.5 text-sm font-medium text-accent"
            >
              {busy ? "Working…" : "Initialize git & use"}
            </motion.button>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
