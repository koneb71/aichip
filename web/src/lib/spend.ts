import { SpendDay, SpendSlice } from "./api";

/**
 * Pure helpers for the spend view.
 *
 * Separate from the component because there is no jsdom in this repo — a
 * rendered component cannot be tested here, so anything with a decision in it
 * lives out here where it can be.
 */

/**
 * The full window, quiet days included.
 *
 * The API only returns days that had runs. Plotting those alone turns two busy
 * days a week apart into two adjacent bars, which reads as "we ran twice in a
 * row" and hides the five days of nothing between them.
 */
export function fillWindow(days: SpendDay[], window: number, today = new Date()): SpendDay[] {
  const byDay = new Map(days.map((d) => [d.day.slice(0, 10), d]));
  return Array.from({ length: window }, (_, i) => {
    const date = new Date(today);
    date.setDate(date.getDate() - (window - 1 - i));
    const key = date.toISOString().slice(0, 10);
    return (
      byDay.get(key) ?? {
        day: key,
        costUsd: 0,
        runs: 0,
        inputTokens: 0,
        outputTokens: 0,
        cacheReadTokens: 0,
      }
    );
  });
}

/**
 * Cache hit rate as a label.
 *
 * `null` is not zero: it means nothing has been sent yet, and printing "0%"
 * for a fresh install claims the cache is broken when it has simply not been
 * asked for anything.
 */
export function cacheHitLabel(rate: number | null): string {
  if (rate === null || Number.isNaN(rate)) return "—";
  return `${Math.round(rate * 100)}%`;
}

/** A slice's own hit rate, for the per-row column. */
export function sliceHitRate(s: SpendSlice): number | null {
  const sent = s.inputTokens + s.cacheReadTokens + s.cacheCreationTokens;
  return sent > 0 ? s.cacheReadTokens / sent : null;
}

/**
 * How many times dearer one thing is than another — "2.6× a plain run".
 *
 * `null` when the baseline is zero or missing: everything is infinitely more
 * expensive than nothing, and saying so helps nobody.
 */
export function multipleLabel(value: number | null, baseline: number | null): string | null {
  if (value === null || baseline === null || baseline <= 0) return null;
  const m = value / baseline;
  // Below about 1.1 the difference is noise dressed up as a finding.
  if (m < 1.1) return null;
  return `${m.toFixed(1)}×`;
}

/** Token counts get long fast; nobody reads "12800000". */
export function compactTokens(n: number): string {
  if (n < 1_000) return String(n);
  if (n < 1_000_000) return `${(n / 1_000).toFixed(n < 10_000 ? 1 : 0)}k`;
  return `${(n / 1_000_000).toFixed(1)}M`;
}

/** What a person calls each run pattern. */
export function patternLabel(key: string): string {
  switch (key) {
    case "task":
      return "Board tasks";
    case "bakeoff":
      return "Bake-offs";
    case "team":
      return "Teams";
    case "chat":
      return "Chat";
    case "workflow":
      return "Workflows";
    case "mention":
      return "Mentions";
    case "knowledge":
      return "Knowledge base";
    case "other":
      return "Other";
    default:
      // A pattern this build has never heard of — a newer server can add one
      // while this page is served from disk. Show it rather than drop it.
      return key;
  }
}
