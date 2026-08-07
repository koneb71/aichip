import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, Project } from "../lib/api";

/**
 * Create a GitHub repository for a project that only exists on this disk.
 *
 * The whole design of this dialog is the confirm. Publishing is outward-facing
 * and the public case cannot be taken back in the way that matters — a push
 * that has been seen has been seen — so nothing here is inferred:
 *
 * - **Private is pre-selected**, and public is a deliberate second click.
 * - **The target is shown before the button does anything.** `gh` resolves the
 *   owner from whoever is signed in, and somebody deciding whether to make
 *   their code public should not have to guess whose account it lands in.
 * - **The name is editable**, defaulted to the folder's, because two projects
 *   cloned from one template are both called `app`.
 */
export function PublishModal({
  projectId,
  onClose,
  onDone,
}: {
  projectId: string;
  onClose: () => void;
  onDone: (repo: string) => void;
}) {
  const [project, setProject] = useState<Project | null>(null);
  const [owner, setOwner] = useState<string | null>(null);
  const [name, setName] = useState("");
  const [isPublic, setIsPublic] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.project(projectId).then((p) => {
      setProject(p);
      setName(suggest(p.path));
    }).catch(() => {});
    // Whose account it lands in, asked rather than assumed — `gh` resolves the
    // owner from whoever is signed in, and that is the part somebody deciding
    // about "public" most needs to see.
    api
      .github()
      .then((g) => setOwner(g.accounts.find((a) => a.active)?.login ?? null))
      .catch(() => {});
  }, [projectId]);

  const publish = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.publishProject(projectId, {
        name: name.trim() || undefined,
        visibility: isPublic ? "public" : "private",
      });
      onDone(r.repo);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
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
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-lg rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">Publish to GitHub</h3>
        <p className="mt-1 text-xs text-ink-dim">
          Creates the repository under your own <code className="font-mono">gh</code> login and
          pushes what is here. Pull requests, issue import and check status work afterwards.
        </p>

        <label className="mt-4 block">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Repository name
          </span>
          <input
            autoFocus
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={busy}
            className="w-full rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-60"
          />
        </label>

        <div className="mt-3">
          <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
            Who can see it
          </span>
          <div className="flex gap-2">
            <Choice
              on={!isPublic}
              onPick={() => setIsPublic(false)}
              disabled={busy}
              label="Private"
              hint="Only you"
            />
            <Choice
              on={isPublic}
              onPick={() => setIsPublic(true)}
              disabled={busy}
              label="Public"
              hint="Anyone on the internet"
            />
          </div>
        </div>

        {/* The whole answer to "what is about to happen", in one line, before
            anything leaves this machine. */}
        {name.trim() && (
          <p className="mt-3 text-[11px] text-ink-dim">
            Will create{" "}
            <span className="font-mono text-ink">
              {owner ? `${owner}/${name.trim()}` : name.trim()}
            </span>{" "}
            as <span className={isPublic ? "font-medium text-amber-700" : "text-ink"}>
              {isPublic ? "public" : "private"}
            </span>
            {project && <> from {project.path}</>}.
          </p>
        )}

        {isPublic && (
          <p className="mt-2 rounded-lg bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-900">
            Everything committed here becomes readable by anyone — including
            anything in the history. Making it private again later does not
            un-publish what was already fetched.
          </p>
        )}

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-danger">
            {error}
          </div>
        )}

        <div className="mt-4 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={publish}
            disabled={busy || !name.trim()}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            {busy ? "Publishing…" : isPublic ? "Create public repository" : "Create private repository"}
          </motion.button>
          <button
            onClick={onClose}
            disabled={busy}
            className="rounded-lg px-3 py-1.5 text-xs text-ink-dim disabled:opacity-50"
          >
            Cancel
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

function Choice({
  on,
  onPick,
  disabled,
  label,
  hint,
}: {
  on: boolean;
  onPick: () => void;
  disabled: boolean;
  label: string;
  hint: string;
}) {
  return (
    <button
      onClick={onPick}
      disabled={disabled}
      className={`flex-1 rounded-lg border px-3 py-2 text-left disabled:opacity-60 ${
        on ? "border-accent bg-accent/5" : "border-line hover:border-ink-dim"
      }`}
    >
      <div className="text-xs font-medium">{label}</div>
      <div className="text-[11px] text-ink-dim">{hint}</div>
    </button>
  );
}

/** The folder's name, made legal — mirrors `publish::suggest_name`. */
function suggest(path: string): string {
  const base = path.replace(/\/+$/, "").split("/").pop() ?? "";
  return base.replace(/[^A-Za-z0-9._-]/g, "-").replace(/^[-.]+/, "");
}
