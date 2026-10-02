import { useEffect, useMemo, useState } from "react";
import { Agent, api, WorkflowDef } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";
import {
  emitWorkflow,
  parseWorkflow,
  Position,
  removeStep,
  StepData,
  uniqueStepId,
  workflowChanged,
  WorkflowMeta,
} from "../../lib/workflowGraph";
import { WorkflowCanvas } from "./WorkflowCanvas";
import { StepInspector } from "./StepInspector";
import { Dialog } from "../ui/Dialog";
import { Button } from "../ui/Button";

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

  // What the editor opened with, written the way it would be saved.
  const savedYaml = useMemo(() => emitWorkflow(initial.meta, initial.steps), [initial]);
  const dirty = workflowChanged(
    { yaml: savedYaml, layout: workflow?.uiLayout ?? {} },
    { yaml, steps, positions },
  );

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
    <Dialog
      open
      onOpenChange={(o) => {
        if (!o) onClose();
      }}
      // Escape is also how people leave a field or a node selection, and here
      // it would throw away every unsaved step.
      dismissible={!dirty}
      width={1152}
      className="top-[7vh]! h-[86vh] max-h-[86vh]!"
      // The name is the dialog's title, and still editable in place.
      title={
        <input
          value={meta.name}
          onChange={(e) => setMeta({ ...meta, name: e.target.value })}
          aria-label="Workflow name"
          className="-ml-2 rounded-lg bg-transparent px-2 py-0.5 text-[15px] font-semibold outline-none hover:bg-panel-2 focus:bg-panel-2"
        />
      }
      footer={
        <>
          {workflow && (
            <Button
              variant="danger"
              className="mr-auto"
              onClick={async () => {
                await api.deleteWorkflow(workflow.id);
                onSaved();
              }}
            >
              Delete workflow
            </Button>
          )}
          <Button variant="ghost" onClick={onClose}>
            Cancel
          </Button>
          <Button variant="primary" onClick={save} disabled={busy}>
            {busy ? "Validating…" : "Save workflow"}
          </Button>
        </>
      }
    >
      {/* Edge to edge: the canvas wants the whole body, not the dialog's
          padded reading column. */}
      <div className="-mx-5 -my-4 flex h-[calc(100%+2rem)] flex-col">
        <header className="flex flex-wrap items-center gap-x-3 gap-y-2 border-b border-border px-4 py-2 sm:px-5">
          <input
            value={meta.schedule ?? ""}
            onChange={(e) => setMeta({ ...meta, schedule: e.target.value || undefined })}
            placeholder="no schedule"
            title="Cron schedule, e.g. 0 3 * * *"
            className="w-32 rounded-lg border border-border bg-panel px-2 py-1 font-mono text-xs outline-none placeholder:text-fg-subtle focus:border-accent"
          />

          <div className="ml-auto flex gap-1 rounded-lg bg-panel-2 p-0.5">
            {(["canvas", "yaml"] as const).map((v) => (
              <button
                key={v}
                onClick={v === "yaml" ? showYaml : showCanvas}
                className={`ring-focus rounded-md px-3 py-1 text-xs capitalize ${
                  view === v ? "bg-panel font-medium shadow-sm" : "text-fg-muted"
                }`}
              >
                {v}
              </button>
            ))}
          </div>
          {view === "canvas" && (
            <Button variant="secondary" size="sm" onClick={addStep}>
              + Step
            </Button>
          )}
        </header>

        {/* Side-by-side once the inspector's 320px still leaves a usable canvas;
            stacked below that, with the canvas keeping the larger share. */}
        <div className="flex min-h-0 flex-1 flex-col lg:grid lg:grid-cols-[minmax(0,1fr)_320px]">
          <div className="min-h-0 min-w-0 basis-3/5 lg:basis-auto">
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
                className="h-full w-full resize-none bg-bg p-5 font-mono text-xs leading-relaxed outline-none"
              />
            )}
          </div>

          <div className="flex min-h-0 min-w-0 flex-1 flex-col border-t border-border lg:border-t-0">
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
            <aside className="min-h-0 overflow-y-auto border-border p-4 lg:border-l">
              <div className="text-[11px] font-semibold uppercase tracking-wide text-fg-muted">
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
                    className="mt-2 w-full resize-none rounded-lg border border-border px-2.5 py-1.5 text-sm outline-none focus:border-accent"
                  />
                  <p className="mt-4 text-xs leading-relaxed text-fg-muted">
                    Drag from a node's right handle to another node's left handle to make it
                    run after. Click a node to edit it. Select an edge and press Delete to
                    unlink.
                  </p>
                  <pre className="mt-4 max-h-64 overflow-auto rounded-lg bg-panel-2 p-2 font-mono text-[10px] leading-relaxed text-fg-muted">
                    {yaml}
                  </pre>
                </>
              ) : (
                <p className="mt-2 text-xs leading-relaxed text-fg-muted">
                  This YAML is what gets saved and committed. Switching back to Canvas
                  re-reads it — comments are not preserved through a canvas edit.
                </p>
              )}
            </aside>
          )}
          </div>
        </div>

        {error && (
          <div className="mx-5 my-2 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">
            {error}
          </div>
        )}
      </div>
    </Dialog>
  );
}
