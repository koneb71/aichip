/**
 * Turning what the CLI said about a plan limit into what a person reads.
 *
 * Pure and in `lib` for the same reason `diff.ts` and `language.ts` are: two
 * surfaces show these limits — the sidebar chip and the usage panel — and a
 * label that disagreed between them would look like two different facts.
 *
 * ## There is no percentage here, and that is not an omission
 *
 * Claude Code's `rate_limit_event` carries a *status*, a window and a reset
 * time. It does not carry how much of the window is spent, and aichip has no
 * other way to know: reading the CLI's config or calling Anthropic are both
 * things this project does not do. So a "68% used" bar would be a number we
 * invented, which is worse than no number. Everything below is derived only
 * from what the CLI actually said.
 */

export type LimitStatus = "allowed" | "warning" | "blocked";

/**
 * `five_hour` is the CLI's word, not a person's. This is the heading form.
 *
 * See [`windowPhrase`] for the mid-sentence form — the two exist because
 * "Weekly usage" is a column header and "nearly out of this week's usage" is a
 * sentence, and lower-casing one into the other produces "out of weekly
 * usage", which is neither.
 */
export function windowLabel(limitType: string): string {
  switch (limitType) {
    case "five_hour":
      return "5-hour window";
    case "seven_day":
      return "Weekly usage";
    default:
      return limitType.replace(/_/g, " ");
  }
}

/** The same window, as it reads after "nearly out of …". */
export function windowPhrase(limitType: string): string {
  switch (limitType) {
    case "five_hour":
      return "this 5-hour window";
    case "seven_day":
      return "this week's usage";
    default:
      return limitType.replace(/_/g, " ");
  }
}

/** What this status means for whether you can start something. */
export function statusLabel(status: string): string {
  switch (status) {
    case "blocked":
      return "Out";
    case "warning":
      return "Nearly out";
    case "allowed":
      return "Fine";
    default:
      return status;
  }
}

/**
 * Colours, as the classes the panel and chip both use.
 *
 * Returned as a pair rather than one string so a caller can put the tone on a
 * dot and the text on its own element without re-deriving it.
 */
export function statusTone(status: string): { text: string; bg: string; dot: string } {
  switch (status) {
    case "blocked":
      return { text: "text-danger", bg: "bg-red-50", dot: "bg-red-500" };
    case "warning":
      return { text: "text-amber-900", bg: "bg-amber-50", dot: "bg-amber-500" };
    default:
      return { text: "text-ink-dim", bg: "bg-panel", dot: "bg-emerald-500" };
  }
}

/**
 * "in 2h", "Thu 3:00 PM" — near things in relative terms, far ones by name.
 *
 * `now` is a parameter rather than `Date.now()` so this is testable without
 * faking the clock, which is the same reason the workflow graph helpers take
 * their inputs.
 */
export function resetIn(iso: string | null, now: number): string | null {
  if (!iso) return null;
  const then = new Date(iso).getTime();
  if (Number.isNaN(then)) return null;
  const mins = Math.round((then - now) / 60_000);
  if (mins <= 0) return "any moment";
  if (mins < 60) return `in ${mins}m`;
  if (mins < 60 * 20) return `in ${Math.round(mins / 60)}h`;
  return new Date(then).toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * A limit whose reset has passed describes a window that has already refilled.
 *
 * The server filters these out of `/api/usage` for the chip, but the panel
 * reads history too, so it needs the same rule — and one implementation of a
 * rule is how the two stay honest with each other.
 */
export function isCurrent(resetsAt: string | null, now: number): boolean {
  if (!resetsAt) return true;
  const t = new Date(resetsAt).getTime();
  return Number.isNaN(t) ? true : t > now;
}

/**
 * One line of history, as a sentence.
 *
 * A transition reads as a change — "nearly out → out" — because that is what
 * was recorded. The first sighting of a limit has no previous state and says
 * so rather than inventing one.
 */
export function transition(previous: string | null, status: string): string {
  if (!previous) return `first seen · ${statusLabel(status).toLowerCase()}`;
  if (previous === status) return `window reset · ${statusLabel(status).toLowerCase()}`;
  return `${statusLabel(previous).toLowerCase()} → ${statusLabel(status).toLowerCase()}`;
}
