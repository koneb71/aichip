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
  // Both of these are the same sentence to a person — the run has stopped and
  // is waiting on them — so neither should read as jargon. "waiting permission"
  // in particular describes what the machine is doing rather than what it wants.
  if (status === "awaiting_approval") return "needs your approval";
  if (status === "waiting_permission") return "needs your answer";
  return status.replace(/_/g, " ");
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

/** How a run's last recorded sentence should be shown, if at all.
 *
 * `runs.error_reason` is not only an error. The permission gate writes
 * "waiting for you to allow Bash" into it *while the run is healthy and
 * parked*, and clears it again on unpark; `finish` coalesces rather than
 * overwrites. So the column is "the last thing said about this run", and
 * keying a red panel off `error !== null` would paint a live, working card red.
 *
 * The pair decides the treatment, in one place, so no component has to
 * re-derive it:
 *
 * - **danger** — it failed, and this is why
 * - **amber** — it was stopped rather than crashed. The attention timeout lands
 *   here ("nobody answered the request to allow Bash after 24h"), which is an
 *   explanation, not a fault
 * - **note** — it is alive and waiting on you; this is a status line
 */
export type StopTone = "danger" | "amber" | "note";

export function stopReason(
  status?: string | null,
  error?: string | null,
): { text: string; tone: StopTone } | null {
  const text = error?.trim();
  if (!text) return null;
  switch (status) {
    case "failed":
      return { text, tone: "danger" };
    case "canceled":
      return { text, tone: "amber" };
    case "waiting_permission":
    case "awaiting_approval":
      return { text, tone: "note" };
    // A run that finished can still have something to say — a real one reads
    // "2 assignments were dropped after failing", which is exactly the kind of
    // partial success that looks like a clean win on the board. It is quiet
    // rather than absent: `unpark` clears the column on the way back to
    // running, so a completed run carrying a stale park note is a bug
    // elsewhere, not the common case this has to defend against.
    case "completed":
      return { text, tone: "note" };
    default:
      return null;
  }
}
