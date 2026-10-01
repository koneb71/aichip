import type { TaskRun } from "./api";
import { compactTokens } from "./spend";

/** Why a run exists, in the words of the person who caused it. */
export function triggerLabel(trigger: string, planFirst = false): string {
  switch (trigger) {
    case "manual":
      return planFirst ? "Plan, then run" : "Run";
    case "resume":
      return "Resumed";
    case "review":
      return "Fix from review";
    case "checks":
      return "Fix failing checks";
    case "conflict":
      return "Resolve conflicts";
    case "bakeoff":
      return "Bake-off variant";
    case "task":
      return "Team run";
    case "schedule":
      return "Scheduled";
    default:
      return trigger.charAt(0).toUpperCase() + trigger.slice(1);
  }
}

/** 41s · 6m 02s · 1h 03m — precise where it matters, short where it doesn't. */
export function duration(seconds: number | null | undefined): string {
  if (seconds == null || seconds < 0) return "—";
  if (seconds < 60) return `${seconds}s`;
  const m = Math.floor(seconds / 60);
  if (m < 60) return `${m}m ${String(seconds % 60).padStart(2, "0")}s`;
  return `${Math.floor(m / 60)}h ${String(m % 60).padStart(2, "0")}m`;
}

/**
 * "12.8k in · 3.1k out · 40k cached", or nothing for a run that never spoke.
 * Provisional counts are the engine's running tally, never confirmed by a
 * final message — marked "≈" so an estimate does not read as a bill.
 */
export function tokensLine(
  run: Pick<
    TaskRun,
    "inputTokens" | "outputTokens" | "cacheReadTokens" | "cacheCreationTokens" | "tokensProvisional"
  >,
): string {
  const cached = run.cacheReadTokens + run.cacheCreationTokens;
  if (run.inputTokens + run.outputTokens + cached === 0) return "";
  const parts = [
    `${compactTokens(run.inputTokens)} in`,
    `${compactTokens(run.outputTokens)} out`,
  ];
  if (cached > 0) parts.push(`${compactTokens(cached)} cached`);
  return (run.tokensProvisional ? "≈ " : "") + parts.join(" · ");
}

/** "$1.84" — or "—" when no run reported dollars (some engines never do). */
export function dollars(cost: number | null | undefined): string {
  if (cost == null) return "—";
  return cost < 0.01 && cost > 0 ? "<$0.01" : `$${cost.toFixed(2)}`;
}
