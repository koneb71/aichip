import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  addEdge,
  Background,
  Connection,
  Controls,
  Edge,
  MarkerType,
  Node,
  NodeChange,
  ReactFlow,
  ReactFlowProvider,
  applyNodeChanges,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import {
  layoutSteps,
  Position,
  StepData,
  wouldCycle,
} from "../../lib/workflowGraph";
import { StepNode, StepNodeData } from "./StepNode";

const nodeTypes = { step: StepNode };

export interface CanvasProps {
  steps: StepData[];
  /** Omitted in read-only (run) mode. */
  onChange?: (steps: StepData[]) => void;
  positions?: Record<string, Position>;
  onPositions?: (positions: Record<string, Position>) => void;
  selectedId?: string | null;
  onSelect?: (id: string | null) => void;
  /** step id → status, when displaying a live run. */
  statuses?: Record<string, { status: string; attempts: number }>;
  readOnly?: boolean;
}

export function WorkflowCanvas(props: CanvasProps) {
  return (
    <ReactFlowProvider>
      <Canvas {...props} />
    </ReactFlowProvider>
  );
}

function Canvas({
  steps,
  onChange,
  positions,
  onPositions,
  selectedId,
  onSelect,
  statuses,
  readOnly,
}: CanvasProps) {
  const [error, setError] = useState<string | null>(null);
  const [dragged, setDragged] = useState<Record<string, Position>>({});
  const laidOut = useMemo(
    () => layoutSteps(steps, { ...positions, ...dragged }),
    [steps, positions, dragged],
  );

  const nodes: Node<StepNodeData>[] = useMemo(
    () =>
      steps.map((step) => ({
        id: step.id,
        type: "step",
        position: laidOut[step.id] ?? { x: 0, y: 0 },
        selected: step.id === selectedId,
        draggable: true,
        data: {
          step,
          status: statuses?.[step.id]?.status,
          attempts: statuses?.[step.id]?.attempts,
        },
      })),
    [steps, laidOut, selectedId, statuses],
  );

  const edges: Edge[] = useMemo(
    () =>
      steps.flatMap((step) =>
        step.needs.map((need) => {
          const active = statuses?.[step.id]?.status;
          return {
            id: `${need}->${step.id}`,
            source: need,
            target: step.id,
            animated: active === "running" || active === "starting",
            style: { stroke: "var(--color-line)", strokeWidth: 1.5 },
            markerEnd: { type: MarkerType.ArrowClosed, color: "var(--color-ink-dim)" },
          };
        }),
      ),
    [steps, statuses],
  );

  // Node drags are position-only; everything else is driven by `steps`.
  const commitTimer = useRef<number | null>(null);
  const onNodesChange = useCallback(
    (changes: NodeChange<Node<StepNodeData>>[]) => {
      const moved = applyNodeChanges(changes, nodes);
      const next: Record<string, Position> = {};
      for (const node of moved) next[node.id] = node.position;
      setDragged(next);

      if (changes.some((c) => c.type === "position" && c.dragging === false)) {
        if (commitTimer.current) window.clearTimeout(commitTimer.current);
        commitTimer.current = window.setTimeout(() => onPositions?.(next), 400);
      }
    },
    [nodes, onPositions],
  );

  useEffect(
    () => () => {
      if (commitTimer.current) window.clearTimeout(commitTimer.current);
    },
    [],
  );

  const onConnect = useCallback(
    (connection: Connection) => {
      if (!onChange || !connection.source || !connection.target) return;
      if (wouldCycle(steps, connection.source, connection.target)) {
        setError("That link would create a loop — steps must flow one way.");
        window.setTimeout(() => setError(null), 3500);
        return;
      }
      onChange(
        steps.map((s) =>
          s.id === connection.target && !s.needs.includes(connection.source!)
            ? { ...s, needs: [...s.needs, connection.source!] }
            : s,
        ),
      );
      // addEdge keeps React Flow's own validation happy about duplicates.
      addEdge(connection, edges);
    },
    [steps, edges, onChange],
  );

  const onEdgesDelete = useCallback(
    (removed: Edge[]) => {
      if (!onChange) return;
      onChange(
        steps.map((s) => {
          const dropped = removed.filter((e) => e.target === s.id).map((e) => e.source);
          return dropped.length ? { ...s, needs: s.needs.filter((n) => !dropped.includes(n)) } : s;
        }),
      );
    },
    [steps, onChange],
  );

  return (
    <div className="relative h-full w-full">
      <ReactFlow
        nodes={nodes}
        edges={edges}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onConnect={onConnect}
        onEdgesDelete={onEdgesDelete}
        onNodeClick={(_, node) => onSelect?.(node.id)}
        onPaneClick={() => onSelect?.(null)}
        nodesConnectable={!readOnly}
        edgesReconnectable={!readOnly}
        elementsSelectable
        deleteKeyCode={readOnly ? null : ["Backspace", "Delete"]}
        fitView
        fitViewOptions={{ padding: 0.25, maxZoom: 1 }}
        proOptions={{ hideAttribution: true }}
        className="bg-surface"
      >
        <Background gap={16} size={1} color="var(--color-line)" />
        <Controls showInteractive={false} className="!shadow-none" />
      </ReactFlow>

      {error && (
        <div className="pointer-events-none absolute inset-x-0 top-3 mx-auto w-fit rounded-lg bg-danger px-3 py-1.5 text-xs text-white shadow">
          {error}
        </div>
      )}
      {steps.length === 0 && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-sm text-ink-dim">
          No steps yet — add one to start building.
        </div>
      )}
    </div>
  );
}
