import { useEffect, useRef, useState } from "react";
import { motion } from "framer-motion";
import { api, type CloneProgress } from "../lib/api";

/**
 * Clone a repository from GitHub into a new project.
 *
 * Polls rather than waits, because a clone of any size takes longer than a
 * request should — the same reason the preview panel polls a build. The poll
 * only runs while a clone is in flight.
 *
 * The one-time cancel matters: a killed clone leaves half a repository on disk,
 * so the server writes into a hidden temporary folder and moves it into place
 * only on success. Cancelling removes it.
 */
export function CloneRepoModal({
  workspaceId,
  onClose,
  onCloned,
}: {
  workspaceId: string;
  onClose: () => void;
  onCloned: (projectId: string) => void;
}) {
  const [repo, setRepo] = useState("");
  const [name, setName] = useState("");
  const [id, setId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const cloneId = useRef<string | null>(null);

  // Only while one is running.
  useEffect(() => {
    if (!id) return;
    const tick = () =>
      api
        .cloneStatus(id)
        .then((p: CloneProgress) => {
          if (p.state === "done") {
            setId(null);
            cloneId.current = null;
            onCloned(p.projectId);
          } else if (p.state === "failed") {
            setId(null);
            cloneId.current = null;
            setBusy(false);
            setError(p.reason);
          }
        })
        .catch(() => {});
    tick();
    const t = setInterval(tick, 2000);
    return () => clearInterval(t);
  }, [id, onCloned]);

  const start = async () => {
    if (!repo.trim()) {
      setError("Paste a repository — owner/repo, or its URL.");
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const started = await api.cloneRepo(
        workspaceId,
        repo.trim(),
        undefined,
        name.trim() || undefined,
      );
      cloneId.current = started.id;
      setId(started.id);
    } catch (e) {
      setBusy(false);
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  };

  const cancel = async () => {
    if (cloneId.current) await api.cancelClone(cloneId.current).catch(() => {});
    onClose();
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={busy ? undefined : onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.97, y: 8 }}
        transition={{ type: "spring", stiffness: 420, damping: 32 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-lg rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">Clone from GitHub</h3>
        <p className="mt-1 text-xs text-ink-dim">
          Cloned with your own <code className="font-mono">gh</code> login — aichip holds no
          credential and never asks for one.
        </p>

        <label className="mt-4 block">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Repository
          </span>
          <input
            autoFocus
            value={repo}
            onChange={(e) => setRepo(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter" && !busy) start();
            }}
            disabled={busy}
            placeholder="owner/repo, or https://github.com/owner/repo"
            className="w-full rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-60"
          />
        </label>

        <label className="mt-3 block">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Folder name <span className="font-normal normal-case">— optional</span>
          </span>
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={busy}
            placeholder="the repository's own name"
            className="w-full rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-60"
          />
        </label>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-danger">
            {error}
          </div>
        )}

        {id && (
          <div className="mt-3 flex items-center gap-2 text-xs text-ink-dim">
            <span className="size-1.5 animate-pulse rounded-full bg-accent" />
            Cloning… this can take a while for a large repository.
          </div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={start}
            disabled={busy}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy ? "Cloning…" : "Clone"}
          </motion.button>
          <button onClick={cancel} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            {busy ? "Stop and discard" : "Cancel"}
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}
