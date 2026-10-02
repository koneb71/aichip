import { useEffect, useState } from "react";
import { api, Project } from "../../lib/api";
import { EnginePicker } from "../../lib/engines";
import { Button } from "../ui/Button";
import { Dialog } from "../ui/Dialog";
import { Select, Textarea } from "../ui/Field";

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
    <Dialog
      open
      onOpenChange={(o) => !o && onClose()}
      width={512}
      title={articleId ? "Ask an agent to revise this page" : "Ask an agent to write it"}
      description={
        <>
          It reads the repository first and writes only what it can verify there.
          {articleId
            ? " You get a proposal to accept or reject — this page does not change on its own."
            : " You get a draft to correct — nothing is published on your behalf."}
        </>
      }
      footer={
        <>
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={go} disabled={!brief.trim() || !projectId || busy}>
            {busy ? "Starting…" : articleId ? "Propose a revision" : "Write it"}
          </Button>
        </>
      }
    >
      <label className="block text-xs font-semibold uppercase tracking-wide text-fg-muted">
        Repository
      </label>
      <Select
        value={projectId}
        onChange={(e) => setProjectId(e.target.value)}
        className="mt-1.5 w-full"
      >
        {projects.map((p) => (
          <option key={p.id} value={p.id}>
            {p.name}
          </option>
        ))}
      </Select>

      <label className="mt-3 block text-xs font-semibold uppercase tracking-wide text-fg-muted">
        {articleId ? "What should change?" : "What should it cover?"}
      </label>
      <Textarea
        value={brief}
        onChange={(e) => setBrief(e.target.value)}
        rows={4}
        placeholder={
          articleId
            ? "The rollback section is out of date — we use make rollback now"
            : "How the queue works and what happens when a run is rate limited"
        }
        className="mt-1.5 resize-none"
      />

      <div className="mt-3">
        <EnginePicker value={engine} onChange={setEngine} inheritLabel="Default engine" />
      </div>

      {error && (
        <div className="mt-3 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
          {error}
        </div>
      )}
    </Dialog>
  );
}
