import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { api, PendingPermission, Task, tierColor, tierModel } from "../lib/api";
import { useRunStream, StreamEvent } from "../lib/ws";
import { Markdown } from "./Markdown";

export function TaskDrawer({
  task,
  onClose,
  onChanged,
}: {
  task: Task;
  onClose: () => void;
  onChanged: () => void;
}) {
  const events = useRunStream(task.runId);
  const [diff, setDiff] = useState<string | null>(null);
  const [merging, setMerging] = useState(false);
  const [serverPending, setServerPending] = useState<PendingPermission[]>([]);
  const [answered, setAnswered] = useState<Set<string>>(new Set());
  const accent = tierColor[task.modelTier];

  // Permission requests are held in memory by the broker while the engine
  // blocks on them, so a refresh has to re-fetch whatever is still open.
  const runId = task.runId;
  const refreshPending = useCallback(async () => {
    if (!runId) return setServerPending([]);
    try {
      setServerPending((await api.pendingPermissions(runId)).pending);
    } catch {
      /* transient; next tick retries */
    }
  }, [runId]);

  useEffect(() => {
    setAnswered(new Set());
    refreshPending();
    const interval = setInterval(refreshPending, 3000);
    return () => clearInterval(interval);
  }, [refreshPending]);

  // Open prompts = server-held ∪ live-streamed, minus resolved/answered.
  const openPermissions = useMemo(() => {
    const resolved = new Set(
      events
        .filter((e) => e.type === "permission_resolved")
        .map((e) => String(e.request_id)),
    );
    const merged = new Map<string, PendingPermission>();
    for (const p of serverPending) merged.set(p.requestId, p);
    for (const e of events) {
      if (e.type !== "permission_requested") continue;
      const requestId = String(e.request_id);
      merged.set(requestId, {
        requestId,
        toolName: String(e.tool_name),
        input: e.input,
      });
    }
    return [...merged.values()].filter(
      (p) => !resolved.has(p.requestId) && !answered.has(p.requestId),
    );
  }, [events, serverPending, answered]);

  const answer = async (requestId: string, allowed: boolean) => {
    setAnswered((prev) => new Set(prev).add(requestId));
    try {
      await api.resolvePermission(requestId, allowed);
    } finally {
      refreshPending();
    }
  };

  const loadDiff = async () => setDiff((await api.diff(task.id)).diff);
  const merge = async () => {
    setMerging(true);
    try {
      await api.merge(task.id);
      onChanged();
      onClose();
    } catch (e) {
      alert(`Merge failed:\n${e}`);
    } finally {
      setMerging(false);
    }
  };

  return (
    <motion.aside
      initial={{ x: 560 }}
      animate={{ x: 0 }}
      exit={{ x: 560 }}
      transition={{ type: "spring", stiffness: 320, damping: 34 }}
      className="card-shadow fixed inset-y-0 right-0 z-30 flex w-[560px] flex-col border-l border-line bg-panel"
    >
      <div className="flex items-start gap-3 border-b border-line p-5">
        <div className="min-w-0 flex-1">
          <div className="truncate text-base font-semibold">{task.title}</div>
          <div className="mt-1 flex items-center gap-2 text-xs text-ink-dim">
            <span
              className="rounded-full px-2 py-0.5"
              style={{ background: `${accent}22`, color: accent }}
            >
              {tierModel[task.modelTier]}
            </span>
            {task.runStatus && <span>{task.runStatus.replace("_", " ")}</span>}
            {task.costUsd != null && <span>${task.costUsd.toFixed(3)}</span>}
          </div>
        </div>
        <button onClick={onClose} className="text-ink-dim hover:text-ink">
          ✕
        </button>
      </div>

      <div className="flex gap-2 border-b border-line px-5 py-3">
        {task.runId && task.runStatus === "running" && (
          <button
            onClick={() => api.cancelRun(task.runId!)}
            className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-red-400 hover:text-red-400"
          >
            Cancel run
          </button>
        )}
        {task.boardColumn === "review" && (
          <>
            <button
              onClick={loadDiff}
              className="rounded-lg border border-line px-3 py-1.5 text-xs hover:border-ink-dim"
            >
              View diff
            </button>
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={merge}
              disabled={merging}
              className="rounded-lg bg-tier-easy px-3 py-1.5 text-xs font-medium text-surface"
            >
              {merging ? "Merging…" : "Squash-merge"}
            </motion.button>
          </>
        )}
      </div>

      <AnimatePresence>
        {openPermissions.length > 0 && (
          <motion.div
            initial={{ height: 0, opacity: 0 }}
            animate={{ height: "auto", opacity: 1 }}
            exit={{ height: 0, opacity: 0 }}
            className="overflow-hidden border-b border-line bg-amber-50"
          >
            <div className="flex flex-col gap-2 p-4">
              {openPermissions.map((p) => (
                <PermissionRow
                  key={p.requestId}
                  toolName={p.toolName}
                  input={p.input}
                  onAnswer={(allowed) => answer(p.requestId, allowed)}
                />
              ))}
            </div>
          </motion.div>
        )}
      </AnimatePresence>

      <div className="min-h-0 flex-1 overflow-y-auto p-5">
        {diff !== null ? (
          <DiffView diff={diff} onBack={() => setDiff(null)} />
        ) : (
          <EventStream events={events} />
        )}
      </div>
    </motion.aside>
  );
}

function EventStream({ events }: { events: StreamEvent[] }) {
  if (events.length === 0) {
    return <div className="text-sm text-ink-dim">No events yet.</div>;
  }
  return (
    <div className="flex flex-col gap-2">
      {events.map((e, i) => (
        <EventRow key={`${e.seq}-${i}`} event={e} />
      ))}
    </div>
  );
}

function EventRow({ event }: { event: StreamEvent }) {
  const base = "rounded-lg px-3 py-2 text-sm";
  switch (event.type) {
    case "run_started":
      return (
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className={`${base} text-xs text-ink-dim`}>
          ▶ session started {String(event.model ?? "")}
        </motion.div>
      );
    case "assistant_text":
      return (
        <motion.div
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          className={`${base} bg-panel-2`}
        >
          <Markdown>{String(event.text)}</Markdown>
        </motion.div>
      );
    case "tool_call":
      return (
        <motion.div
          initial={{ opacity: 0, y: 4 }}
          animate={{ opacity: 1, y: 0 }}
          className={`${base} border border-line font-mono text-xs text-ink-dim`}
        >
          ⚙ {String(event.tool_name)}{" "}
          <span className="opacity-70">
            {JSON.stringify(event.input).slice(0, 140)}
          </span>
        </motion.div>
      );
    case "tool_result":
      return (
        <div className={`${base} font-mono text-xs ${event.is_error ? "text-red-400" : "text-ink-dim/80"}`}>
          ↳ {String(event.summary).slice(0, 200)}
        </div>
      );
    // Open prompts render in the sticky banner above; the log keeps only a
    // trace so the transcript stays readable.
    case "permission_requested":
      return (
        <div className={`${base} text-xs text-ink-dim`}>
          ⏸ asked to run {String(event.tool_name)}
        </div>
      );
    case "permission_resolved":
      return (
        <div className={`${base} text-xs text-ink-dim`}>
          {event.allowed ? "✓ you allowed it" : "✗ you denied it"}
        </div>
      );
    case "run_completed":
      return (
        <motion.div
          initial={{ scale: 0.97, opacity: 0 }}
          animate={{ scale: 1, opacity: 1 }}
          className={`${base} border border-tier-easy/40 bg-tier-easy/10 text-tier-easy`}
        >
          <div className="flex gap-1.5">
            <span>✓</span>
            <Markdown>{String(event.result_text)}</Markdown>
          </div>
        </motion.div>
      );
    case "run_failed":
      return (
        <div className={`${base} border border-red-400/40 bg-red-400/10 text-red-400`}>
          ✗ {String(event.reason)}
        </div>
      );
    case "rate_limited":
      return (
        <div className={`${base} border border-amber-400/40 bg-amber-400/10 text-amber-600`}>
          ⏳ rate-limited — re-queued automatically
        </div>
      );
    default:
      return null;
  }
}

function PermissionRow({
  toolName,
  input,
  onAnswer,
}: {
  toolName: string;
  input: unknown;
  onAnswer: (allowed: boolean) => void;
}) {
  const summary = summarizeToolInput(toolName, input);
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      className="rounded-xl border border-amber-300 bg-panel px-3 py-2.5"
    >
      <div className="text-sm font-medium text-amber-700">
        Allow <span className="font-mono">{toolName}</span>?
      </div>
      {summary && (
        <pre className="mt-1.5 max-h-32 overflow-auto rounded-lg bg-panel-2 p-2 font-mono text-xs text-ink">
          {summary}
        </pre>
      )}
      <div className="mt-2.5 flex gap-2">
        <motion.button
          whileTap={{ scale: 0.95 }}
          onClick={() => onAnswer(true)}
          className="rounded-lg bg-tier-easy px-3.5 py-1.5 text-xs font-medium text-white"
        >
          Allow
        </motion.button>
        <motion.button
          whileTap={{ scale: 0.95 }}
          onClick={() => onAnswer(false)}
          className="rounded-lg border border-line px-3.5 py-1.5 text-xs hover:border-danger hover:text-danger"
        >
          Deny
        </motion.button>
      </div>
    </motion.div>
  );
}

/** Show the part of a tool call the user actually needs to judge. */
function summarizeToolInput(toolName: string, input: unknown): string {
  const args = (input ?? {}) as Record<string, unknown>;
  if (typeof args.command === "string") return args.command;
  if (typeof args.file_path === "string") {
    const body = typeof args.content === "string" ? `\n\n${args.content}` : "";
    return `${args.file_path}${body}`.slice(0, 1200);
  }
  const json = JSON.stringify(args, null, 1);
  return json === "{}" ? "" : json.slice(0, 1200);
}

function DiffView({ diff, onBack }: { diff: string; onBack: () => void }) {
  return (
    <div>
      <button onClick={onBack} className="mb-3 text-xs text-ink-dim hover:text-ink">
        ← back to stream
      </button>
      <pre className="overflow-x-auto rounded-lg bg-panel-2 p-3 font-mono text-xs leading-relaxed">
        {diff.split("\n").map((line, i) => (
          <div
            key={i}
            className={
              line.startsWith("+") && !line.startsWith("+++")
                ? "text-tier-easy"
                : line.startsWith("-") && !line.startsWith("---")
                  ? "text-red-400"
                  : line.startsWith("@@")
                    ? "text-tier-medium"
                    : "text-ink-dim"
            }
          >
            {line || " "}
          </div>
        ))}
      </pre>
    </div>
  );
}
