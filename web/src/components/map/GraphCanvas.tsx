import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Background,
  Controls,
  Edge,
  MarkerType,
  Node,
  ReactFlow,
  ReactFlowProvider,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { GraphEdge, GraphFile, layout, moduleOf, neighbourhood } from "../../lib/repoGraph";
import { MapNodeData, nodeTypes } from "./GraphNodes";

/**
 * The project, drawn.
 *
 * Everything about *where things go* is in `lib/repoGraph.ts` and tested
 * there; this file only turns those coordinates into React Flow's shapes and
 * handles the clicking. That split is not tidiness — there is no jsdom in this
 * repository, so logic that lives in a `.tsx` can never be asserted about.
 *
 * Nodes are not draggable. The workflow canvas lets you arrange steps because
 * a workflow is something you author; this is something you read, and a
 * position a person moved would then be a claim about the code that the code
 * did not make.
 */
export function GraphCanvas({
  files,
  edges,
  selected,
  onSelect,
  onOpenFile,
}: {
  files: GraphFile[];
  edges: GraphEdge[];
  selected: string | null;
  onSelect: (id: string | null) => void;
  onOpenFile: (path: string) => void;
}) {
  const [expanded, setExpanded] = useState<Set<string>>(new Set());

  // A module that stops existing must not stay in the open set, or reopening
  // the tab after a refactor draws a disc around nothing.
  useEffect(() => {
    const live = new Set(files.map((f) => moduleOf(f.path)));
    setExpanded((prev) => {
      const next = new Set([...prev].filter((m) => live.has(m)));
      return next.size === prev.size ? prev : next;
    });
  }, [files]);

  const laid = useMemo(() => layout(files, edges, expanded), [files, edges, expanded]);

  // One hop from the selection, in both directions. Precomputed once per
  // selection rather than per node, which matters on a graph whose busiest
  // file has forty-six importers.
  const near = useMemo(
    () => (selected ? neighbourhood(selected, laid.edges) : null),
    [selected, laid.edges],
  );

  const rfNodes: Node<MapNodeData>[] = useMemo(
    () =>
      laid.nodes.map((n) => ({
        id: n.id,
        type: n.kind,
        // React Flow positions from a node's top-left; every coordinate here
        // is a centre, because that is what a disc has.
        position: { x: n.x - n.r, y: n.y - n.r },
        draggable: false,
        selectable: true,
        data: {
          node: n,
          near: !near || near.has(n.id),
          focused: n.id === selected,
          dimmed: !!near && !near.has(n.id),
        },
      })),
    [laid.nodes, near, selected],
  );

  const rfEdges: Edge[] = useMemo(
    () =>
      laid.edges.map((e) => {
        const lit = !near || (near.has(e.from) && near.has(e.to));
        return {
          id: e.id,
          source: e.from,
          target: e.to,
          // Edges are SVG paths React Flow owns; a Tailwind stroke class on
          // them does nothing, so colour goes through `style`.
          style: {
            stroke: e.cyclic ? "var(--color-danger)" : "var(--color-ink-dim)",
            strokeWidth: e.width,
            strokeDasharray: e.cyclic ? "4 3" : undefined,
            opacity: lit ? 0.55 : 0.06,
          },
          label: e.weight > 1 ? String(e.weight) : undefined,
          labelStyle: { fontSize: 9, fill: "var(--color-ink-dim)" },
          labelBgStyle: { fill: "var(--color-panel)" },
          labelShowBg: true,
          markerEnd: {
            type: MarkerType.ArrowClosed,
            width: 12,
            height: 12,
            color: e.cyclic ? "var(--color-danger)" : "var(--color-ink-dim)",
          },
        };
      }),
    [laid.edges, near],
  );

  const click = useCallback(
    (id: string) => {
      const node = laid.nodes.find((n) => n.id === id);
      if (!node) return;
      if (node.kind === "module") {
        setExpanded((prev) => {
          const next = new Set(prev);
          if (next.has(id)) next.delete(id);
          else next.add(id);
          return next;
        });
        onSelect(null);
        return;
      }
      onSelect(id === selected ? null : id);
    },
    [laid.nodes, onSelect, selected],
  );

  return (
    <ReactFlowProvider>
      <div className="relative h-full min-h-0 w-full min-w-0">
        <ReactFlow
          nodes={rfNodes}
          edges={rfEdges}
          nodeTypes={nodeTypes}
          onNodeClick={(_, node) => click(node.id)}
          onNodeDoubleClick={(_, node) => {
            const n = laid.nodes.find((x) => x.id === node.id);
            if (n?.kind === "file") onOpenFile(n.id);
          }}
          onPaneClick={() => onSelect(null)}
          nodesConnectable={false}
          nodesDraggable={false}
          elementsSelectable
          deleteKeyCode={null}
          minZoom={0.15}
          fitView
          fitViewOptions={{ padding: 0.2, maxZoom: 1.1 }}
          proOptions={{ hideAttribution: true }}
          className="bg-surface"
        >
          <Background gap={16} size={1} color="var(--color-line)" />
          <Controls showInteractive={false} className="!shadow-none" />
        </ReactFlow>

        {laid.cycles.length > 0 && (
          <div className="pointer-events-none absolute left-3 top-3 rounded-lg bg-red-50 px-2 py-1 text-[10px] text-danger">
            {laid.cycles.length} circular dependenc
            {laid.cycles.length === 1 ? "y" : "ies"} between modules — drawn dashed
          </div>
        )}
        {files.length === 0 && (
          <div className="pointer-events-none absolute inset-0 flex items-center justify-center text-sm text-ink-dim">
            Nothing read yet.
          </div>
        )}
      </div>
    </ReactFlowProvider>
  );
}
