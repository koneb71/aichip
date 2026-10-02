import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { api, Effort, Project, TierChoice } from "../lib/api";
import { EnginePicker } from "../lib/engines";
import { TIERS } from "./TierPicker";
import { ChecksSettings } from "./ChecksSettings";
import { ReviewPolicySettings } from "./ReviewPolicySettings";
import { Dialog } from "./ui/Dialog";
import { Button } from "./ui/Button";

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
    // Closing is refused while a save or unload is in flight, as the scrim
    // click was before: the result has nowhere to land once this is gone.
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
      title="Project settings"
      description={<span className="block truncate font-mono text-[11px]">{project.path}</span>}
      width={512}
    >
      <Field label="Name">
        <div className="flex gap-2">
          <input
            value={name}
            onChange={(e) => setName(e.target.value)}
            disabled={busy}
            className="min-w-0 flex-1 rounded-lg border border-border bg-bg px-2 py-1.5 text-sm outline-none focus:border-accent disabled:opacity-60"
          />
          <Button
            size="sm"
            onClick={() => save({ name: name.trim() })}
            disabled={busy || !name.trim() || name === project.name}
          >
            Rename
          </Button>
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
              className="min-w-0 flex-1 rounded-lg border border-border bg-bg px-2 py-1.5 font-mono text-sm outline-none focus:border-accent disabled:opacity-60"
            />
            <Button
              size="sm"
              onClick={() => save({ default_branch: branch.trim() })}
              disabled={busy || !branch.trim() || branch === project.defaultBranch}
            >
              Save
            </Button>
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
            className="rounded-lg border border-border bg-panel px-2 py-1 text-xs disabled:opacity-60"
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
            className="rounded-lg border border-border bg-panel px-2 py-1 text-xs disabled:opacity-60"
          >
            <option value="">Effort: inherit</option>
            <option value="low">low</option>
            <option value="medium">medium</option>
            <option value="high">high</option>
          </select>
        </div>
      </Field>

      {project.kind === "repo" && project.vcs === "git" && (
        <Field label="Checks">
          <ChecksSettings projectId={project.id} fullAuto={project.fullAutoOptIn} />
        </Field>
      )}

      {project.kind === "repo" && project.vcs === "git" && (
        <Field label="Before merge">
          <ReviewPolicySettings projectId={project.id} workspaceId={project.workspaceId} />
        </Field>
      )}

      {error && (
        <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 text-[11px] leading-relaxed text-danger-fg">
          {error}
        </div>
      )}

      <div className="mt-5 border-t border-border pt-4">
        {!confirmUnload ? (
          <Button variant="danger" size="sm" onClick={() => setConfirmUnload(true)} disabled={busy}>
            Unload this project
          </Button>
        ) : (
          <div className="rounded-lg bg-warning-subtle px-3 py-2.5 text-[11px] leading-relaxed text-warning-fg">
            {/* Said first and plainly. "Remove" next to a filesystem path
                reads as "delete my code", and this is the one thing somebody
                needs to be sure of before clicking. */}
            <div className="font-medium">
              Your folder stays exactly where it is. Nothing on disk is deleted.
            </div>
            <div className="mt-1">
              Eren forgets this project: its cards, their runs and comments, its chats,
              and the <code className="font-mono">eren/*</code> branches and checkouts
              Eren created for it. Load the folder again to start over.
            </div>
            <div className="mt-2 flex items-center gap-2">
              <Button variant="danger" size="sm" onClick={unload} disabled={busy}>
                {busy ? "Unloading…" : "Unload it"}
              </Button>
              <Button variant="ghost" size="sm" onClick={() => setConfirmUnload(false)} disabled={busy}>
                Keep it
              </Button>
            </div>
          </div>
        )}
      </div>

      <div className="mt-4">
        <Button variant="ghost" size="sm" onClick={onClose}>
          Done
        </Button>
      </div>
    </Dialog>
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
      <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
        {label}
      </span>
      {children}
      {hint && <p className="mt-1 text-[11px] text-fg-muted/80">{hint}</p>}
    </div>
  );
}
