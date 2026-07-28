import { motion } from "framer-motion";

/**
 * One "may I?" from a running agent, with the answer buttons.
 *
 * Shared between the task drawer (where prompts appear beside the run that
 * raised them) and the activity page (where they appear as the thing
 * blocking the whole workspace). Same decision either way, so it must look
 * and behave identically — a permission prompt that renders differently in
 * two places is a prompt people learn to click through.
 */
export function PermissionRow({
  toolName,
  input,
  context,
  onAnswer,
}: {
  toolName: string;
  input: unknown;
  /** Which run is asking. Only shown where that isn't already obvious. */
  context?: string;
  onAnswer: (allowed: boolean) => void;
}) {
  const summary = summarizeToolInput(toolName, input);
  return (
    <motion.div
      initial={{ opacity: 0, scale: 0.98 }}
      animate={{ opacity: 1, scale: 1 }}
      className="rounded-xl border border-amber-300 bg-panel px-3 py-2.5"
    >
      {context && <div className="mb-1 truncate text-xs text-ink-dim">{context}</div>}
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
export function summarizeToolInput(toolName: string, input: unknown): string {
  const args = (input ?? {}) as Record<string, unknown>;
  if (typeof args.command === "string") return args.command;
  if (typeof args.file_path === "string") {
    const body = typeof args.content === "string" ? `\n\n${args.content}` : "";
    return `${args.file_path}${body}`.slice(0, 1200);
  }
  const json = JSON.stringify(args, null, 1);
  return json === "{}" ? "" : json.slice(0, 1200);
}
