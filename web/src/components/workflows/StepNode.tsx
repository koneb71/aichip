import { Handle, NodeProps, Position } from "@xyflow/react";
import { motion } from "framer-motion";
import { Tier, tierColor, tierModel, tierSoft } from "../../lib/api";
import { StepData } from "../../lib/workflowGraph";

export interface StepNodeData extends Record<string, unknown> {
  step: StepData;
  /** Live status when the canvas is showing a run rather than an editor. */
  status?: string;
  attempts?: number;
}

const TIERS = new Set(["easy", "medium", "complex"]);

export function StepNode({ data, selected }: NodeProps & { data: StepNodeData }) {
  const { step, status, attempts } = data;
  const tier = TIERS.has(step.model ?? "") ? (step.model as Tier) : undefined;
  const accent = tier ? tierColor[tier] : "var(--color-ink-dim)";
  const running = status === "running" || status === "starting";
  const parallel = step.parallel ?? 1;

  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.96 }}
      animate={{ opacity: 1, scale: 1 }}
      className="w-56 rounded-xl border bg-panel px-3 py-2.5 text-left"
      style={{
        borderColor: selected
          ? "var(--color-accent)"
          : status
            ? statusColor(status)
            : "var(--color-line)",
        boxShadow: running
          ? `0 0 0 3px ${statusColor(status!)}22`
          : "0 1px 2px rgba(16,17,20,0.06)",
      }}
    >
      <Handle
        type="target"
        position={Position.Left}
        className="!h-2.5 !w-2.5 !border-2 !border-panel !bg-ink-dim"
      />

      <div className="flex items-center gap-1.5">
        {status && <StatusPip status={status} />}
        <span className="min-w-0 flex-1 truncate font-mono text-sm font-medium">
          {step.id}
        </span>
        {parallel > 1 && (
          <span
            className="shrink-0 rounded-full bg-panel-2 px-1.5 py-0.5 text-[10px] text-ink-dim"
            title={`${parallel} attempts run in parallel`}
          >
            ×{parallel}
          </span>
        )}
      </div>

      <p className="mt-1 line-clamp-2 text-[11px] leading-snug text-ink-dim">
        {step.prompt.trim() || "No prompt yet"}
      </p>

      <div className="mt-2 flex flex-wrap items-center gap-1">
        {tier && (
          <span
            className="rounded-full px-1.5 py-0.5 text-[10px]"
            style={{ background: tierSoft[tier], color: accent }}
          >
            {tierModel[tier]}
          </span>
        )}
        {step.model && !tier && (
          <span className="rounded-full bg-panel-2 px-1.5 py-0.5 font-mono text-[10px] text-ink-dim">
            {step.model}
          </span>
        )}
        {step.agent && (
          <span className="rounded-full bg-tier-medium-soft px-1.5 py-0.5 text-[10px] text-tier-medium">
            {step.agent}
          </span>
        )}
        {step.session === "continue" && (
          <span
            className="rounded-full bg-panel-2 px-1.5 py-0.5 text-[10px] text-ink-dim"
            title="Resumes the previous step's session"
          >
            ↻ session
          </span>
        )}
        {step.isolatedWorktrees && parallel > 1 && (
          <span
            className="rounded-full bg-panel-2 px-1.5 py-0.5 text-[10px] text-ink-dim"
            title="Each attempt gets its own worktree"
          >
            isolated
          </span>
        )}
        {attempts != null && attempts > 1 && (
          <span className="rounded-full bg-panel-2 px-1.5 py-0.5 text-[10px] text-ink-dim">
            {attempts} attempts
          </span>
        )}
      </div>

      <Handle
        type="source"
        position={Position.Right}
        className="!h-2.5 !w-2.5 !border-2 !border-panel !bg-ink-dim"
      />
    </motion.div>
  );
}

export function statusColor(status: string): string {
  switch (status) {
    case "completed":
      return "var(--color-tier-easy)";
    case "failed":
      return "var(--color-danger)";
    case "canceled":
      return "var(--color-ink-dim)";
    default:
      return "var(--color-tier-medium)";
  }
}

function StatusPip({ status }: { status: string }) {
  const color = statusColor(status);
  const live = status === "running" || status === "starting" || status === "queued";
  return live ? (
    <motion.span
      className="h-2 w-2 shrink-0 rounded-full"
      style={{ background: color }}
      animate={{ opacity: [1, 0.3, 1] }}
      transition={{ repeat: Infinity, duration: 1.6 }}
    />
  ) : (
    <span className="h-2 w-2 shrink-0 rounded-full" style={{ background: color }} />
  );
}
