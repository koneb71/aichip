import { useEffect, useMemo, useState } from "react";
import { motion } from "framer-motion";
import { Agent, api, WorkflowDef } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";
import {
  emitWorkflow,
  parseWorkflow,
  Position,
  removeStep,
  StepData,
  uniqueStepId,
  WorkflowMeta,
} from "../../lib/workflowGraph";
import { WorkflowCanvas } from "./WorkflowCanvas";
import { StepInspector } from "./StepInspector";

const STARTER = `name: plan-build-review
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
  const { active } = useWorkspace();
  const initial = useMemo(
    () => parseWorkflow(workflow?.sourceYaml ?? STARTER),
    [workflow],
  );

  const [meta, setMeta] = useState<WorkflowMeta>(initial.meta);
  const [steps, setSteps] = useState<StepData[]>(initial.steps);
  const [positions, setPositions] = useState<Record<string, Position>>(
    workflow?.uiLayout ?? {},
  );
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [view, setView] = useState<"canvas" | "yaml">("canvas");
  const [rawYaml, setRawYaml] = useState(workflow?.sourceYaml ?? STARTER);
  const [agents, setAgents] = useState<Agent[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (!active) return;
    api.agents(active.id).then((r) => setAgents(r.agents)).catch(() => {});
  }, [active]);

  // The canvas is authoritative while it's showing; switching to YAML
  // renders what would be saved.
  const yaml = view === "yaml" ? rawYaml : emitWorkflow(meta, steps);

  const showYaml = () => {
    setRawYaml(emitWorkflow(meta, steps));
    setView("yaml");
  };
  const showCanvas = () => {
    const parsed = parseWorkflow(rawYaml);
    setMeta(parsed.meta);
    setSteps(parsed.steps);
    setView("canvas");
  };

  const addStep = () => {
    const id = uniqueStepId(steps, "step");
    const last = steps[steps.length - 1];
    setSteps([
      ...steps,
      {
        id,
        prompt: "",
        // Chain onto the end by default — that's the common case, and an
        // unlinked node is one drag away anyway.
        needs: last ? [last.id] : [],
        model: "medium",
      },
    ]);
    setSelectedId(id);
  };

  const save = async () => {
    setBusy(true);
    setError(null);
    const source = view === "yaml" ? rawYaml : emitWorkflow(meta, steps);
    try {
      const saved = workflow
        ? await api.updateWorkflow(workflow.id, source)
        : await api.createWorkflow(projectId, source);
      await api.saveWorkflowLayout(saved.id, positions).catch(() => {});
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const selected = steps.find((s) => s.id === selectedId) ?? null;

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
        className="card-shadow flex h-[86vh] w-full max-w-6xl flex-col overflow-hidden rounded-2xl border border-line bg-panel"
      >
        <header className="flex items-center gap-3 border-b border-line px-5 py-3">
          <input
            value={meta.name}
            onChange={(e) => setMeta({ ...meta, name: e.target.value })}
            className="rounded-lg px-2 py-1 text-base font-semibold outline-none hover:bg-panel-2 focus:bg-panel-2"
          />
          <input
            value={meta.schedule ?? ""}
            onChange={(e) => setMeta({ ...meta, schedule: e.target.value || undefined })}
            placeholder="no schedule"
            title="Cron schedule, e.g. 0 3 * * *"
            className="w-32 rounded-lg border border-line px-2 py-1 font-mono text-xs outline-none focus:border-accent"
          />

          <div className="ml-auto flex gap-1 rounded-lg bg-panel-2 p-0.5">
            {(["canvas", "yaml"] as const).map((v) => (
              <button
                key={v}
                onClick={v === "yaml" ? showYaml : showCanvas}
                className={`rounded-md px-3 py-1 text-xs capitalize ${
                  view === v ? "bg-panel font-medium shadow-sm" : "text-ink-dim"
                }`}
              >
                {v}
              </button>
            ))}
          </div>
          {view === "canvas" && (
            <button
              onClick={addStep}
              className="rounded-lg border border-line px-3 py-1.5 text-xs hover:bg-panel-2"
            >
              + Step
            </button>
          )}
        </header>

        <div className="grid min-h-0 flex-1 grid-cols-[1fr_320px]">
          {view === "canvas" ? (
            <WorkflowCanvas
              steps={steps}
              onChange={setSteps}
              positions={positions}
              onPositions={setPositions}
              selectedId={selectedId}
              onSelect={setSelectedId}
            />
          ) : (
            <textarea
              value={rawYaml}
              onChange={(e) => setRawYaml(e.target.value)}
              spellCheck={false}
              className="h-full w-full resize-none bg-surface p-5 font-mono text-xs leading-relaxed outline-none"
            />
          )}

          {selected && view === "canvas" ? (
            <StepInspector
              step={selected}
              steps={steps}
              agents={agents}
              onChange={setSteps}
              onDelete={() => {
                setSteps(removeStep(steps, selected.id));
                setSelectedId(null);
              }}
            />
          ) : (
            <aside className="overflow-y-auto border-l border-line p-4">
              <div className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
                {view === "canvas" ? "Workflow" : "Preview"}
              </div>
              {view === "canvas" ? (
                <>
                  <textarea
                    value={meta.description ?? ""}
                    onChange={(e) =>
                      setMeta({ ...meta, description: e.target.value || undefined })
                    }
                    rows={3}
                    placeholder="What does this workflow do?"
                    className="mt-2 w-full resize-none rounded-lg border border-line px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                  />
                  <p className="mt-4 text-xs leading-relaxed text-ink-dim">
                    Drag from a node's right handle to another node's left handle to make it
                    run after. Click a node to edit it. Select an edge and press Delete to
                    unlink.
                  </p>
                  <pre className="mt-4 max-h-64 overflow-auto rounded-lg bg-panel-2 p-2 font-mono text-[10px] leading-relaxed text-ink-dim">
                    {yaml}
                  </pre>
                </>
              ) : (
                <p className="mt-2 text-xs leading-relaxed text-ink-dim">
                  This YAML is what gets saved and committed. Switching back to Canvas
                  re-reads it — comments are not preserved through a canvas edit.
                </p>
              )}
            </aside>
          )}
        </div>

        {error && (
          <div className="mx-5 mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
            {error}
          </div>
        )}

        <footer className="flex items-center justify-between border-t border-line p-4">
          {workflow ? (
            <button
              onClick={async () => {
                await api.deleteWorkflow(workflow.id);
                onSaved();
              }}
              className="text-sm text-danger hover:underline"
            >
              Delete workflow
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
        </footer>
      </motion.div>
    </motion.div>
  );
}
