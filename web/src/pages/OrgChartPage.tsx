import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate } from "react-router-dom";
import {
  Background,
  Controls,
  Handle,
  MarkerType,
  Position,
  ReactFlow,
  ReactFlowProvider,
  useNodesState,
  type Edge,
  type Node,
  type NodeProps,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { HeartPulse, Network } from "lucide-react";
import { api } from "../lib/api";
import { BOX, layoutTree, OrgNode, todayLine, wouldCycle } from "../lib/orgChart";
import { useWorkspace } from "../lib/workspace";
import { useTheme } from "../lib/theme";
import { Page } from "../components/ui/Surface";
import { EmptyState, PageHeader } from "../components/ui/Layout";
import { Avatar } from "../components/ui/Avatar";
import { StatusDot } from "../components/ui/Badge";
import { Dialog } from "../components/ui/Dialog";
import { Button } from "../components/ui/Button";
import { toast } from "../components/ui/Toast";

/**
 * Who reports to whom, and what each agent is doing right now.
 *
 * Drag an agent onto another to make it report there; the server keeps the
 * chart a tree (no loops, one workspace, a bounded depth) and this page asks
 * before it sends. Click an agent to open it.
 */
export default function OrgChartPage() {
  const { active } = useWorkspace();
  const [nodes, setNodes] = useState<OrgNode[] | null>(null);
  const [loads, setLoads] = useState(0);

  useEffect(() => {
    if (!active) return;
    let stale = false;
    api
      .orgChart(active.id)
      .then((r) => !stale && setNodes(r.nodes))
      .catch(() => !stale && setNodes([]));
    return () => {
      stale = true;
    };
  }, [active, loads]);

  // Live: who is working on what changes by the minute.
  useEffect(() => {
    const t = setInterval(() => {
      if (document.visibilityState !== "hidden") setLoads((n) => n + 1);
    }, 15_000);
    return () => clearInterval(t);
  }, []);

  return (
    <Page wide>
      <PageHeader
        title="Org chart"
        icon={<Network className="size-4" />}
        description="Who reports to whom. A manager agent hands work only down its own branch, and hears about trouble from it. Drag an agent onto another to move it."
      />
      {nodes === null ? (
        <div className="skeleton h-[520px] rounded-xl" />
      ) : nodes.length === 0 ? (
        <EmptyState title="No agents yet" hint="Create agents first, then arrange who reports to whom here." />
      ) : (
        <ReactFlowProvider>
          <Chart nodes={nodes} onChanged={() => setLoads((n) => n + 1)} />
        </ReactFlowProvider>
      )}
    </Page>
  );
}

type CardData = { node: OrgNode; reports: number };

function Chart({ nodes, onChanged }: { nodes: OrgNode[]; onChanged: () => void }) {
  const navigate = useNavigate();
  const { theme } = useTheme();
  const [move, setMove] = useState<{ agent: OrgNode; to: OrgNode | null } | null>(null);
  const [busy, setBusy] = useState(false);

  const laid = useMemo(() => layoutTree(nodes), [nodes]);
  const byId = useMemo(() => new Map(nodes.map((n) => [n.id, n])), [nodes]);
  const flowNodes: Node<CardData>[] = useMemo(
    () =>
      nodes.map((n) => ({
        id: n.id,
        type: "agent",
        position: laid.positions.get(n.id) ?? { x: 0, y: 0 },
        data: { node: n, reports: nodes.filter((m) => m.reportsTo === n.id).length },
        draggable: true,
      })),
    [nodes, laid],
  );
  // Local copies, so a box follows the cursor while dragged; reset from the
  // layout whenever the chart changes (or a drop is cancelled).
  const [rfNodes, setRfNodes, onNodesChange] = useNodesState(flowNodes);
  useEffect(() => setRfNodes(flowNodes), [flowNodes, setRfNodes]);
  const snapBack = useCallback(() => setRfNodes(flowNodes), [flowNodes, setRfNodes]);
  const flowEdges: Edge[] = useMemo(
    () =>
      laid.edges.map((e) => ({
        id: `${e.from}-${e.to}`,
        source: e.from,
        target: e.to,
        type: "smoothstep",
        markerEnd: { type: MarkerType.ArrowClosed, width: 14, height: 14 },
        style: { stroke: "var(--color-border-strong)" },
      })),
    [laid],
  );

  // A drop onto another box is a request to report there; anywhere else is
  // nothing, and the layout puts the box back.
  const onNodeDragStop = useCallback(
    (_: unknown, dragged: Node<CardData>) => {
      const agent = byId.get(dragged.id);
      if (!agent) return;
      const cx = dragged.position.x + BOX.w / 2;
      const cy = dragged.position.y + BOX.h / 2;
      const target = nodes.find((n) => {
        if (n.id === agent.id) return false;
        const p = laid.positions.get(n.id);
        return p && cx >= p.x && cx <= p.x + BOX.w && cy >= p.y && cy <= p.y + BOX.h;
      });
      if (!target || target.id === agent.reportsTo) {
        snapBack();
        return;
      }
      if (wouldCycle(nodes, agent.id, target.id)) {
        toast("That would make a loop", { tone: "danger", body: `${target.name} already reports to ${agent.name}, directly or not.` });
        snapBack();
        return;
      }
      setMove({ agent, to: target });
    },
    [byId, nodes, laid, snapBack],
  );

  const confirm = async () => {
    if (!move) return;
    setBusy(true);
    try {
      await api.updateAgent(move.agent.id, { reports_to: move.to?.id ?? null });
      toast(move.to ? `${move.agent.name} now reports to ${move.to.name}` : `${move.agent.name} is at the top`, { tone: "success" });
    } catch (e) {
      toast("Not moved", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(false);
      setMove(null);
      onChanged();
    }
  };

  return (
    <div className="h-[calc(100vh-220px)] min-h-[480px] overflow-hidden rounded-xl border border-border bg-bg">
      <ReactFlow
        nodes={rfNodes}
        onNodesChange={onNodesChange}
        edges={flowEdges}
        nodeTypes={NODE_TYPES}
        colorMode={theme}
        fitView
        fitViewOptions={{ padding: 0.2, maxZoom: 1 }}
        minZoom={0.2}
        nodesConnectable={false}
        onNodeDragStop={onNodeDragStop}
        onNodeClick={(_, n) => navigate(`/agents?agent=${n.id}`)}
        proOptions={{ hideAttribution: true }}
      >
        <Background gap={20} size={1} color="var(--color-border)" />
        <Controls showInteractive={false} />
      </ReactFlow>
      <Dialog
        open={move !== null}
        onOpenChange={(o) => !o && (setMove(null), snapBack())}
        title="Change who they report to?"
        description={
          move
            ? move.to
              ? `${move.agent.name} will report to ${move.to.name}. A manager hands work only down its own branch, so this changes who can give ${move.agent.name} cards.`
              : `${move.agent.name} will be at the top of the chart.`
            : ""
        }
        width={440}
      >
        <div className="flex justify-end gap-2">
          <Button variant="ghost" onClick={() => (setMove(null), snapBack())}>
            Cancel
          </Button>
          <Button variant="primary" loading={busy} onClick={() => void confirm()}>
            Move
          </Button>
        </div>
      </Dialog>
    </div>
  );
}

const TONE = { active: "success", paused: "warning", retired: "neutral", pending_approval: "info" } as const;

function AgentCard({ data }: NodeProps<Node<CardData>>) {
  const n = data.node;
  const today = todayLine(n);
  const beating = n.heartbeatSecs !== null;
  return (
    <div
      className="cursor-pointer rounded-lg border border-border bg-raised px-3 py-2.5 text-left shadow-[var(--shadow-sm)] transition-colors hover:border-border-strong"
      style={{ width: BOX.w, minHeight: BOX.h }}
    >
      <Handle type="target" position={Position.Top} className="!opacity-0" isConnectable={false} />
      <div className="flex items-center gap-2">
        <Avatar name={n.name} color={n.color} size={26} />
        <div className="min-w-0 flex-1">
          <div className="flex items-center gap-1.5">
            <span className="truncate text-[13px] font-medium text-fg">{n.name}</span>
            <StatusDot tone={TONE[n.status]} pulse={!!n.liveCard} label={n.status} />
          </div>
          <div className="truncate text-[11px] text-fg-muted">{n.title || "No title"}</div>
        </div>
        {beating && (
          <HeartPulse
            className="size-3.5 shrink-0 text-danger-fg"
            aria-label={`Heartbeat every ${Math.round((n.heartbeatSecs ?? 0) / 60)} minutes`}
          />
        )}
      </div>
      <div className="mt-2 truncate font-mono text-[11px] text-fg-subtle">
        {n.engine ?? "card's engine"} · {n.modelTier}
        {data.reports > 0 ? ` · ${data.reports} report${data.reports === 1 ? "" : "s"}` : ""}
      </div>
      <div className="mt-1 truncate text-[11px] text-fg-muted">
        {n.liveCard ? <>Working on “{n.liveCard.title}”</> : today ?? "Idle today"}
      </div>
      <Handle type="source" position={Position.Bottom} className="!opacity-0" isConnectable={false} />
    </div>
  );
}

const NODE_TYPES = { agent: AgentCard };
