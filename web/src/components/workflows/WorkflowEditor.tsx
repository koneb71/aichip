import { useState } from "react";
import { motion } from "framer-motion";
import { api, WorkflowDef } from "../../lib/api";

const TEMPLATE = `name: plan-build-review
description: Plan a change, implement it, then review the result
defaults:
  engine: claude-code
  permission_mode: auto_edit
steps:
  - id: plan
    model: complex
    prompt: |
      Study this repository and write a short implementation plan for:
      <describe the change here>
  - id: build
    needs: [plan]
    model: medium
    session: continue
    prompt: |
      Implement the plan you just wrote:
      {{ steps.plan.output }}
  - id: review
    needs: [build]
    model: complex
    prompt: |
      Review the changes just made. Report any bug, missing test, or
      regression you find. If it looks good, say so plainly.
`;

export function WorkflowEditor({
  projectId,
  workflow,
  onClose,
  onSaved,
}: {
  projectId: string;
  workflow: WorkflowDef | null;
  onClose: () => void;
  onSaved: () => void;
}) {
  const [yaml, setYaml] = useState(workflow?.sourceYaml ?? TEMPLATE);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      if (workflow) await api.updateWorkflow(workflow.id, yaml);
      else await api.createWorkflow(projectId, yaml);
      onSaved();
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
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-6"
      onClick={onClose}
    >
      <motion.div
        initial={{ y: 20, scale: 0.98 }}
        animate={{ y: 0, scale: 1 }}
        exit={{ y: 20, scale: 0.98 }}
        transition={{ type: "spring", stiffness: 380, damping: 30 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow flex h-[80vh] w-full max-w-3xl flex-col rounded-2xl border border-line bg-panel"
      >
        <div className="border-b border-line p-5">
          <div className="text-base font-semibold">
            {workflow ? `Edit ${workflow.name}` : "New workflow"}
          </div>
          <div className="mt-0.5 text-xs text-ink-dim">
            Steps run in dependency order. Use{" "}
            <code className="rounded bg-panel-2 px-1">needs</code> to chain them,{" "}
            <code className="rounded bg-panel-2 px-1">
              {"{{ steps.<id>.output }}"}
            </code>{" "}
            to pass results, and{" "}
            <code className="rounded bg-panel-2 px-1">strategy.parallel</code> to fan out.
          </div>
        </div>

        <textarea
          value={yaml}
          onChange={(e) => setYaml(e.target.value)}
          spellCheck={false}
          className="min-h-0 flex-1 resize-none bg-transparent p-5 font-mono text-xs leading-relaxed outline-none"
        />

        {error && (
          <div className="mx-5 mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        <div className="flex items-center justify-between border-t border-line p-4">
          {workflow ? (
            <button
              onClick={async () => {
                await api.deleteWorkflow(workflow.id);
                onSaved();
              }}
              className="text-sm text-danger hover:underline"
            >
              Delete
            </button>
          ) : (
            <span />
          )}
          <div className="flex gap-2">
            <button
              onClick={onClose}
              className="rounded-lg px-4 py-2 text-sm text-ink-dim hover:text-ink"
            >
              Cancel
            </button>
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={save}
              disabled={busy}
              className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
            >
              {busy ? "Validating…" : "Save workflow"}
            </motion.button>
          </div>
        </div>
      </motion.div>
    </motion.div>
  );
}
