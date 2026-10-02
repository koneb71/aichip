import { motion } from "framer-motion";
import { Button } from "./ui/Button";

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
      className="rounded-xl border border-warning/40 bg-panel px-3 py-2.5"
    >
      {context && <div className="mb-1 truncate text-xs text-fg-muted">{context}</div>}
      <div className="text-sm font-medium text-warning-fg">
        Allow <span className="font-mono">{toolName}</span>?
      </div>
      {summary && (
        <pre className="mt-1.5 max-h-32 overflow-auto rounded-lg bg-panel-2 p-2 font-mono text-xs text-fg">
          {summary}
        </pre>
      )}
      <div className="mt-2.5 flex gap-2">
        <Button variant="primary" size="sm" onClick={() => onAnswer(true)}>
          Allow
        </Button>
        <Button variant="secondary" size="sm" onClick={() => onAnswer(false)}>
          Deny
        </Button>
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
