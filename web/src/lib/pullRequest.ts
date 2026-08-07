/**
 * Reading a pull request's state as one line.
 *
 * Pure and in `lib` for the same reason `usage.ts` is: the drawer row and the
 * board chip must agree, and a label that disagreed between them would look
 * like two different pull requests.
 */

import type { TaskPullRequest } from "./api";

/**
 * The single worst thing true about a pull request, which is what a chip has
 * room for.
 *
 * Ordered by what stops you: merged is the end of the story and outranks
 * everything; a failing check outranks an approval, because approved code that
 * does not build is not ready; a draft is never "ready" however green it is.
 */
export function prSummary(pr: TaskPullRequest): string {
  if (pr.state === "merged") return "merged";
  if (pr.state === "closed") return "closed";
  if (pr.checks === "failing") return "checks failing";
  if (pr.state === "draft") return "draft";
  if (pr.checks === "pending") return "checks running";
  if (pr.review === "changes_requested") return "changes requested";
  if (pr.review === "approved") return "approved";
  return "open";
}

/** The colour that summary carries, as the classes both surfaces use. */
export function prTone(pr: TaskPullRequest): { text: string; dot: string } {
  const summary = prSummary(pr);
  if (summary === "merged") return { text: "text-tier-complex", dot: "bg-tier-complex" };
  if (summary === "checks failing" || summary === "changes requested") {
    return { text: "text-danger", dot: "bg-red-500" };
  }
  if (summary === "closed") return { text: "text-ink-dim", dot: "bg-ink-dim" };
  if (summary === "approved") return { text: "text-tier-easy", dot: "bg-emerald-500" };
  if (summary === "checks running") return { text: "text-amber-900", dot: "bg-amber-500" };
  return { text: "text-ink-dim", dot: "bg-ink-dim" };
}

/**
 * How old the cached answer is.
 *
 * `now` is a parameter so this is testable without faking the clock. A
 * timestamp in the future — which clock skew between the server and the
 * browser produces routinely — reads as "just now" rather than a negative
 * duration, because "synced in −3 minutes" is not a thing that can be true.
 */
export function syncedLabel(syncedAt: string | null, now: number): string {
  if (!syncedAt) return "never checked";
  const then = new Date(syncedAt).getTime();
  if (Number.isNaN(then)) return "never checked";
  const secs = Math.round((now - then) / 1000);
  if (secs < 45) return "just now";
  if (secs < 3600) return `${Math.round(secs / 60)}m ago`;
  if (secs < 86_400) return `${Math.round(secs / 3600)}h ago`;
  return `${Math.round(secs / 86_400)}d ago`;
}

/**
 * Whether to keep asking.
 *
 * Only while something is actually in flight — the same rule the preview panel
 * follows. A merged pull request will never change again, and polling a card
 * nobody is looking at is a `gh` process per interval forever.
 */
export function shouldPoll(pr: TaskPullRequest | null): boolean {
  if (!pr) return false;
  if (pr.state !== "open" && pr.state !== "draft") return false;
  return pr.checks === "pending";
}
