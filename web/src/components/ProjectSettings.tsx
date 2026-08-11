import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { motion } from "framer-motion";
import { api, Effort, Project, TierChoice } from "../lib/api";
import { EnginePicker } from "../lib/engines";
import { TIERS } from "./TierPicker";

/**
 * Everything about a project that is not a card.
 *
 * All of this was reachable only through the database before: the name was the
 * folder's basename forever, `default_branch` was accepted by the API and never
 * sent by anything, and a project could not be removed at all — the only
 * cascade that dropped one was deleting its whole workspace, which has no UI
 * either. Load the wrong folder once and it was in the sidebar for good.
 */
export function ProjectSettings({
  project,
  onChanged,
  onClose,
}: {
  project: Project;
  onChanged: (p: Project) => void;
  onClose: () => void;
}) {
  const navigate = useNavigate();
  const [name, setName] = useState(project.name);
  const [branch, setBranch] = useState(project.defaultBranch);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [confirmUnload, setConfirmUnload] = useState(false);

  const save = async (body: Parameters<typeof api.updateProject>[1]) => {
    setBusy(true);
    setError(null);
    try {
      onChanged(await api.updateProject(project.id, body));
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const unload = async () => {
    setBusy(true);
    try {
      await api.unloadProject(project.id);
      navigate("/projects");
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={busy ? undefined : onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/25 backdrop-blur-[3px] p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 12, opacity: 0 }}
        animate={{ scale: 1, y: 0, opacity: 1 }}
        transition={{ type: "spring", stiffness: 220, damping: 26 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow max-h-[85vh] w-full max-w-lg overflow-y-auto rounded-2xl bg-panel p-5"
      >
        <h3 className="text-sm font-semibold">Project settings</h3>
        <p className="mt-0.5 truncate font-mono text-[11px] text-ink-dim">{project.path}</p>

        <Field label="Name">
          <div className="flex gap-2">
            <input
              value={name}
              onChange={(e) => setName(e.target.value)}
              disabled={busy}
              className="min-w-0 flex-1 rounded-lg border border-line bg-surface px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-60"
            />
            <button
              onClick={() => save({ name: name.trim() })}
              disabled={busy || !name.trim() || name === project.name}
              className="shrink-0 rounded-lg border border-line px-2.5 py-1.5 text-xs hover:border-ink-dim disabled:opacity-40"
            >
              Rename
            </button>
          </div>
        </Field>

        {project.vcs === "git" && (
          <Field
            label="Base branch"
            hint="What cards branch from and merge back into. Change this if the repository's branch was renamed."
          >
            <div className="flex gap-2">
              <input
                value={branch}
                onChange={(e) => setBranch(e.target.value)}
                disabled={busy}
                className="min-w-0 flex-1 rounded-lg border border-line bg-surface px-2 py-1.5 font-mono text-sm outline-none focus:border-accent disabled:opacity-60"
              />
              <button
                onClick={() => save({ default_branch: branch.trim() })}
                disabled={busy || !branch.trim() || branch === project.defaultBranch}
                className="shrink-0 rounded-lg border border-line px-2.5 py-1.5 text-xs hover:border-ink-dim disabled:opacity-40"
              >
                Save
              </button>
            </div>
          </Field>
        )}

        <Field
          label="What new cards start on"
          hint="Leave any of these on Inherit to keep deciding per card."
        >
          <div className="flex flex-wrap items-center gap-2">
            <EnginePicker
              value={project.defaultEngine ?? null}
              onChange={(e) => save({ default_engine: e })}
              inheritLabel="Engine: inherit"
            />
            <select
              value={project.defaultTier ?? ""}
              onChange={(e) =>
                save({ default_tier: (e.target.value || null) as TierChoice | null })
              }
              disabled={busy}
              className="rounded-lg border border-line bg-panel px-2 py-1 text-xs disabled:opacity-60"
            >
              <option value="">Model: inherit</option>
              <option value="auto">auto</option>
              {TIERS.map((t) => (
                <option key={t} value={t}>
                  {t}
                </option>
              ))}
            </select>
            <select
              value={project.defaultEffort ?? ""}
              onChange={(e) =>
                save({ default_effort: (e.target.value || null) as Effort | null })
              }
              disabled={busy}
              className="rounded-lg border border-line bg-panel px-2 py-1 text-xs disabled:opacity-60"
            >
              <option value="">Effort: inherit</option>
              <option value="low">low</option>
              <option value="medium">medium</option>
              <option value="high">high</option>
            </select>
          </div>
        </Field>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-[11px] leading-relaxed text-danger">
            {error}
          </div>
        )}

        <div className="mt-5 border-t border-line pt-4">
          {!confirmUnload ? (
            <button
              onClick={() => setConfirmUnload(true)}
              disabled={busy}
              className="rounded-lg border border-line px-3 py-1.5 text-xs text-ink-dim hover:border-danger hover:text-danger disabled:opacity-50"
            >
              Unload this project
            </button>
          ) : (
            <div className="rounded-lg bg-amber-50 px-3 py-2.5 text-[11px] leading-relaxed text-amber-900">
              {/* Said first and plainly. "Remove" next to a filesystem path
                  reads as "delete my code", and this is the one thing somebody
                  needs to be sure of before clicking. */}
              <div className="font-medium">
                Your folder stays exactly where it is. Nothing on disk is deleted.
              </div>
              <div className="mt-1">
                aichip forgets this project: its cards, their runs and comments, its chats,
                and the <code className="font-mono">aichip/*</code> branches and checkouts
                aichip created for it. Load the folder again to start over.
              </div>
              <div className="mt-2 flex items-center gap-2">
                <button
                  onClick={unload}
                  disabled={busy}
                  className="rounded-lg bg-danger px-2.5 py-1 text-xs font-medium text-white disabled:opacity-60"
                >
                  {busy ? "Unloading…" : "Unload it"}
                </button>
                <button
                  onClick={() => setConfirmUnload(false)}
                  disabled={busy}
                  className="px-2 py-1 text-xs text-amber-900/80"
                >
                  Keep it
                </button>
              </div>
            </div>
          )}
        </div>

        <div className="mt-4">
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Done
          </button>
        </div>
      </motion.div>
    </motion.div>
  );
}

function Field({
  label,
  hint,
  children,
}: {
  label: string;
  hint?: string;
  children: React.ReactNode;
}) {
  return (
    <div className="mt-4">
      <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
        {label}
      </span>
      {children}
      {hint && <p className="mt-1 text-[11px] text-ink-dim/80">{hint}</p>}
    </div>
  );
}
