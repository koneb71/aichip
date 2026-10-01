import type { CheckResult, CheckStatus, LocalChecksSummary } from "./api";

export type CheckTone = "good" | "bad" | "busy" | "warn";

/** The card's chip: what its checks say, at a glance. Null when there is nothing worth a chip. */
export function checksChip(
  c: LocalChecksSummary | null | undefined,
): { label: string; tone: CheckTone; title: string } | null {
  if (!c) return null;
  switch (c.status) {
    case "passed":
      return { label: `✓ ${c.passed}/${c.total}`, tone: "good", title: "This project's checks pass on this card" };
    case "failed":
      return {
        label: `✗ ${c.passed}/${c.total}`,
        tone: "bad",
        title: `${c.total - c.passed} of ${c.total} checks fail on this card`,
      };
    case "queued":
    case "running":
      return { label: "◌ checks", tone: "busy", title: "Running this project's checks" };
    case "error":
      return { label: "checks ?", tone: "warn", title: "The checks could not run — open the card for why" };
    default:
      // Canceled means newer work replaced it; the newer result speaks for itself.
      return null;
  }
}

/** Is this status still going? */
export function checksActive(status: CheckStatus | null | undefined): boolean {
  return status === "queued" || status === "running";
}

export function resultPassed(r: CheckResult): boolean {
  return !r.timedOut && r.exitCode === 0;
}

/** "exit 1", "timed out", "could not start", "passed". */
export function resultVerdict(r: CheckResult): string {
  if (r.timedOut) return "timed out";
  if (r.exitCode === null) return "could not start";
  return r.exitCode === 0 ? "passed" : `exit ${r.exitCode}`;
}
