/**
 * Run status predicates, mirroring `RunStatus` in aichip-shared.
 *
 * These used to be spelled out inline at half a dozen call sites, which is
 * how a new status (`awaiting_approval`) silently reads as "dead" in one
 * place and "working" in another.
 */

const TERMINAL = new Set(["completed", "failed", "canceled"]);
const WORKING = new Set(["starting", "running"]);

export type RunStatusName =
  | "queued"
  | "starting"
  | "running"
  | "waiting_permission"
  | "awaiting_approval"
  | "rate_limited"
  | "completed"
  | "failed"
  | "canceled";

export function isTerminal(status?: string | null): boolean {
  return !!status && TERMINAL.has(status);
}

/** Still owes an outcome, even if nothing is executing right now. */
export function isActive(status?: string | null): boolean {
  return !!status && !TERMINAL.has(status);
}

/** An engine is burning tokens *now* — narrower than active. A parked or
 *  rate-limited run must not animate as though someone is typing. */
export function isWorking(status?: string | null): boolean {
  return !!status && WORKING.has(status);
}

/** Waiting on a person rather than on a model. */
export function needsYou(status?: string | null): boolean {
  return status === "awaiting_approval" || status === "waiting_permission";
}

/** Human-readable label; snake_case reads badly in a chip. */
export function statusLabel(status?: string | null): string {
  if (!status) return "";
  return status === "awaiting_approval" ? "needs your approval" : status.replace(/_/g, " ");
}

export function statusColor(status?: string | null): string {
  switch (status) {
    case "completed":
      return "var(--color-tier-easy)";
    case "failed":
      return "var(--color-danger)";
    case "canceled":
    case "skipped":
      return "var(--color-ink-dim)";
    case "awaiting_approval":
    case "waiting_permission":
      return "#d97706"; // amber: blocked on the user
    default:
      return "var(--color-tier-medium)";
  }
}
