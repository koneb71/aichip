import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, type Project, type RepoApp } from "../../lib/api";

/**
 * Apps a project offers under `.aichip/apps/`.
 *
 * How a team shares one: commit the manifest and it arrives in a pull request
 * like anything else. Nothing syncs on its own and nothing is watched —
 * installing something from a repository is a thing a person does, and doing it
 * automatically would mean a `git pull` could add tables.
 *
 * A broken manifest is listed with its error rather than skipped: "three of
 * four" with no explanation is how someone spends an afternoon looking for the
 * fourth.
 */
export function RepoApps({
  workspaceId,
  onSynced,
}: {
  workspaceId: string;
  onSynced: () => void;
}) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState("");
  const [found, setFound] = useState<RepoApp[] | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .projects(workspaceId)
      .then((r) => setProjects(r.projects))
      .catch(() => setProjects([]));
  }, [workspaceId]);

  useEffect(() => {
    if (!projectId) {
      setFound(null);
      return;
    }
    setError(null);
    api
      .repoApps(projectId)
      .then((r) => setFound(r.apps))
      .catch((e) => {
        setFound([]);
        setError(String(e).replace(/^Error:\s*/, ""));
      });
  }, [projectId]);

  const sync = async (app: RepoApp) => {
    setBusy(app.dir);
    setError(null);
    try {
      await api.syncRepoApp(projectId, app.dir);
      onSynced();
      // Re-read so an app that was new now says it is installed.
      setFound((await api.repoApps(projectId)).apps);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(null);
    }
  };

  if (projects.length === 0) return null;

  return (
    <div className="mt-8">
      <div className="mb-2 flex items-center gap-3">
        <span className="text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
          From a repository
        </span>
        <select
          value={projectId}
          onChange={(e) => setProjectId(e.target.value)}
          className="rounded-lg border border-line bg-surface px-2 py-1 text-xs outline-none focus:border-accent"
        >
          <option value="">Choose a project…</option>
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>
        <span className="text-[11px] text-ink-dim">
          Anything committed under <span className="font-mono">.aichip/apps/</span>.
        </span>
      </div>

      {error && (
        <div className="mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      {found?.length === 0 && (
        <div className="rounded-xl border border-dashed border-line p-6 text-center text-xs text-ink-dim">
          This project has no apps in <span className="font-mono">.aichip/apps/</span>. Export one
          as <strong>Share</strong> and commit its manifest there to offer it to everyone working
          on this repository.
        </div>
      )}

      <div className="flex flex-col gap-1">
        {(found ?? []).map((a) => (
          <div
            key={a.dir}
            className="flex items-center gap-3 rounded-lg border border-line px-3 py-2 text-xs"
          >
            <div className="min-w-0 flex-1">
              <div className="truncate font-medium">{a.name}</div>
              <div className="truncate text-[11px] text-ink-dim">
                {a.error ? (
                  <span className="text-danger">{a.error}</span>
                ) : (
                  a.summary || <span className="font-mono">{a.dir}</span>
                )}
              </div>
            </div>
            {/* "Update" rather than "Install" when it is already here: syncing
                replaces the manifest of the app of that name and keeps its
                rows, which is a different promise and worth a different word. */}
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={() => sync(a)}
              disabled={busy !== null || a.error !== null}
              title={
                a.error
                  ? "This manifest does not parse, so there is nothing to install."
                  : a.installedAs
                    ? "Replace the installed app's manifest. Its rows are kept."
                    : "Install it here."
              }
              className="shrink-0 rounded-lg border border-line px-2 py-1 hover:bg-line/40 disabled:opacity-40"
            >
              {busy === a.dir ? "…" : a.installedAs ? "Update" : "Install"}
            </motion.button>
          </div>
        ))}
      </div>
    </div>
  );
}
