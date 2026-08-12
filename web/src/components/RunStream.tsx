import { motion } from "framer-motion";
import { StreamEvent } from "../lib/ws";
import { Markdown } from "./Markdown";

/**
 * What a run is doing, rendered the same way everywhere.
 *
 * This used to live inside the task drawer, which meant a board task showed a
 * full transcript while a team run showed only status chips — you could watch
 * one agent work and could not see the other five at all. The renderer is
 * shared so "what is it doing?" has one answer regardless of which surface
 * you happen to be looking at.
 */
export function RunStream({
  events,
  stepId,
  empty = "Nothing yet.",
}: {
  events: StreamEvent[];
  /** Show only this step's events — one teammate in a team run, one node in a
   *  workflow. Omit for the whole run. */
  stepId?: string;
  empty?: string;
}) {
  const shown = stepId ? events.filter((e) => eventStep(e) === stepId) : events;

  if (shown.length === 0) {
    return <div className="text-sm text-ink-dim">{empty}</div>;
  }
  return (
    <div className="flex flex-col gap-2">
      {shown.map((e, i) => (
        <EventRow key={`${e.seq}-${i}`} event={e} />
      ))}
    </div>
  );
}

/** Replay frames and live frames disagree on casing; both are in play. */
export function eventStep(e: StreamEvent): string | undefined {
  return (e.stepId as string) ?? (e.step_id as string) ?? undefined;
}

/**
 * The stream reduced to one line: what is happening *right now*.
 *
 * A status of "running" tells you a process exists. This tells you it is
 * three minutes into a `docker compose up`, which is the difference between
 * trusting the thing and staring at a spinner.
 */
export function lastActivity(events: StreamEvent[], stepId?: string): string | null {
  const shown = stepId ? events.filter((e) => eventStep(e) === stepId) : events;
  for (let i = shown.length - 1; i >= 0; i--) {
    const e = shown[i];
    switch (e.type) {
      case "tool_call":
        return `${e.tool_name} ${summarizeInput(e.input)}`.trim();
      case "assistant_text": {
        const text = String(e.text ?? "").trim().replace(/\s+/g, " ");
        if (text) return text.length > 90 ? `${text.slice(0, 90)}…` : text;
        break;
      }
      case "permission_requested":
        return `waiting on you: ${e.tool_name}`;
      case "run_completed":
        return "finished";
      case "run_failed":
        // Deliberately just the fact. The reason has its own panel now, in
        // full, next to the status word it explains — and this line is
        // *what it is doing*, clipped to 60 characters and set in mono. Two
        // truncations of one sentence, one of them worse, is not two views.
        return "failed";
      case "rate_limited":
        return "rate limited";
      case "run_started":
        return "starting up";
    }
  }
  return null;
}

/** The part of a tool call worth putting on one line. */
function summarizeInput(input: unknown): string {
  const args = (input ?? {}) as Record<string, unknown>;
  const pick =
    (typeof args.command === "string" && args.command) ||
    (typeof args.file_path === "string" && args.file_path) ||
    (typeof args.pattern === "string" && args.pattern) ||
    (typeof args.path === "string" && args.path) ||
    "";
  const one = String(pick).replace(/\s+/g, " ").trim();
  return one.length > 60 ? `${one.slice(0, 60)}…` : one;
}

function EventRow({ event }: { event: StreamEvent }) {
  const base = "rounded-lg px-3 py-2 text-sm";
  switch (event.type) {
    case "run_started":
      return (
        <motion.div
          initial={{ opacity: 0 }}
          animate={{ opacity: 1 }}
          className={`${base} text-xs text-ink-dim`}
        >
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
          <span className="opacity-70">{JSON.stringify(event.input).slice(0, 140)}</span>
        </motion.div>
      );
    case "tool_result":
      return (
        <div
          className={`${base} font-mono text-xs ${
            event.is_error ? "text-red-400" : "text-ink-dim/80"
          }`}
        >
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
        <div className={`${base} border border-amber-300 bg-amber-50 text-xs text-amber-800`}>
          ⏳ {String(event.message)}
        </div>
      );
    default:
      return null;
  }
}

/**
 * A compact "…is doing X" line with a live pulse.
 *
 * Used on board cards, the activity page and each teammate in a team run, so
 * the same glance answers the same question in all three places.
 */
export function ActivityLine({
  events,
  stepId,
  live,
  className = "",
}: {
  events: StreamEvent[];
  stepId?: string;
  live: boolean;
  className?: string;
}) {
  const what = lastActivity(events, stepId);
  if (!what) return null;
  return (
    <div className={`flex min-w-0 items-center gap-1.5 text-[11px] text-ink-dim ${className}`}>
      {live && (
        <motion.span
          className="h-1 w-1 shrink-0 rounded-full bg-tier-medium"
          animate={{ opacity: [1, 0.25, 1] }}
          transition={{ duration: 1.6, repeat: Infinity }}
        />
      )}
      <span className="truncate font-mono">{what}</span>
    </div>
  );
}
