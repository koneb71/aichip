import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, Project } from "../../lib/api";
import { EnginePicker } from "../../lib/engines";

/**
 * Ask an agent to write or revise a page.
 *
 * With `articleId`, this is a revision: the result lands as a **proposal** for
 * a person to accept, never as a silent replacement. Without one, it creates a
 * new draft.
 */
export function GenerateModal({
  workspaceId,
  articleId,
  defaultProjectId,
  parentId,
  onClose,
  onStarted,
}: {
  workspaceId: string;
  /** Present means "revise this page" rather than "write a new one". */
  articleId?: string;
  defaultProjectId?: string | null;
  parentId?: string | null;
  onClose: () => void;
  onStarted: (newPageId?: string) => void;
}) {
  const [projects, setProjects] = useState<Project[]>([]);
  const [projectId, setProjectId] = useState(defaultProjectId ?? "");
  const [brief, setBrief] = useState("");
  const [engine, setEngine] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api
      .projects(workspaceId)
      .then((r) => {
        setProjects(r.projects);
        setProjectId((id) => id || r.projects[0]?.id || "");
      })
      .catch(() => {});
  }, [workspaceId]);

  const go = async () => {
    if (!projectId || !brief.trim() || busy) return;
    setBusy(true);
    setError(null);
    try {
      if (articleId) {
        await api.reviseArticle(articleId, {
          project_id: projectId,
          brief: brief.trim(),
          engine: engine ?? undefined,
        });
        onStarted();
      } else {
        const r = await api.generateArticle({
          workspace_id: workspaceId,
          project_id: projectId,
          brief: brief.trim(),
          engine: engine ?? undefined,
          parent_id: parentId ?? undefined,
        });
        // Hand the id back so the caller can open the page and watch it being
        // written, rather than leaving the user to find it in the tree.
        onStarted(r.articleId ?? undefined);
      }
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
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow w-full max-w-lg rounded-2xl border border-line bg-panel p-6"
      >
        <div className="text-lg font-semibold">
          {articleId ? "Ask an agent to revise this page" : "Ask an agent to write it"}
        </div>
        <p className="mt-1 text-sm text-ink-dim">
          It reads the repository first and writes only what it can verify there.
          {articleId
            ? " You get a proposal to accept or reject — this page does not change on its own."
            : " You get a draft to correct — nothing is published on your behalf."}
        </p>

        <label className="mt-4 block text-xs font-semibold uppercase tracking-wide text-ink-dim">
          Repository
        </label>
        <select
          value={projectId}
          onChange={(e) => setProjectId(e.target.value)}
          className="mt-1.5 w-full rounded-lg border border-line bg-panel px-2.5 py-2 text-sm"
        >
          {projects.map((p) => (
            <option key={p.id} value={p.id}>
              {p.name}
            </option>
          ))}
        </select>

        <label className="mt-3 block text-xs font-semibold uppercase tracking-wide text-ink-dim">
          {articleId ? "What should change?" : "What should it cover?"}
        </label>
        <textarea
          value={brief}
          onChange={(e) => setBrief(e.target.value)}
          rows={4}
          placeholder={
            articleId
              ? "The rollback section is out of date — we use make rollback now"
              : "How the queue works and what happens when a run is rate limited"
          }
          className="mt-1.5 w-full resize-none rounded-lg border border-line bg-panel px-3 py-2 text-sm outline-none focus:border-accent"
        />

        <div className="mt-3">
          <EnginePicker value={engine} onChange={setEngine} inheritLabel="Default engine" />
        </div>

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        <div className="mt-5 flex justify-end gap-2">
          <button onClick={onClose} className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink">
            Cancel
          </button>
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={go}
            disabled={!brief.trim() || !projectId || busy}
            className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
          >
            {busy ? "Starting…" : articleId ? "Propose a revision" : "Write it"}
          </motion.button>
        </div>
      </motion.div>
    </motion.div>
  );
}
