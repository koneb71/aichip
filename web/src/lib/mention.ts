/**
 * Parsing for `@`-mentions of project files.
 *
 * Kept out of the component so the fiddly parts — where a token starts, what
 * counts as a line range — are unit-tested rather than only clickable, and so
 * that typing `@api.ts:10-25` by hand behaves exactly like picking it in the UI.
 */

export interface MentionToken {
  /** Index of the `@`. */
  start: number;
  /** Path part: what the file search is called with. */
  query: string;
  /** Text after the first `:`, or null when no `:` has been typed yet. */
  lineQuery: string | null;
}

export interface LineSpec {
  start: number;
  /** Absent for a single line. */
  end?: number;
}

/**
 * The active mention at the caret, or null.
 *
 * A token starts at an `@` that is either at the start of the text or preceded
 * by whitespace — that exclusion is what stops `user@example.com` from opening
 * a file picker — and runs to the caret with no whitespace in between.
 */
export function mentionToken(text: string, caret: number): MentionToken | null {
  const upto = text.slice(0, caret);
  const at = upto.lastIndexOf("@");
  if (at === -1) return null;
  if (at > 0 && !/\s/.test(upto[at - 1])) return null;

  const body = upto.slice(at + 1);
  if (/\s/.test(body)) return null;

  const colon = body.indexOf(":");
  return colon === -1
    ? { start: at, query: body, lineQuery: null }
    : { start: at, query: body.slice(0, colon), lineQuery: body.slice(colon + 1) };
}

/**
 * Normalize a typed line spec: `"42"` → a single line, `"10-25"` → a range.
 * Returns null for anything that isn't a positive line number, and swaps a
 * reversed range so `25-10` means the same as `10-25`.
 */
export function parseLineSpec(spec: string): LineSpec | null {
  const trimmed = spec.trim();
  if (!trimmed) return null;

  const range = trimmed.match(/^(\d+)-(\d+)$/);
  if (range) {
    const a = Number(range[1]);
    const b = Number(range[2]);
    if (a < 1 || b < 1) return null;
    const [start, end] = a <= b ? [a, b] : [b, a];
    return start === end ? { start } : { start, end };
  }

  if (!/^\d+$/.test(trimmed)) return null;
  const n = Number(trimmed);
  return n >= 1 ? { start: n } : null;
}

/** Render a mention as it appears in the prompt. */
export function formatMention(path: string, lines?: LineSpec): string {
  if (!lines) return `\`${path}\``;
  const suffix = lines.end && lines.end !== lines.start
    ? `${lines.start}-${lines.end}`
    : `${lines.start}`;
  return `\`${path}:${suffix}\``;
}

/**
 * Replace the mention token spanning `[start, caret)` with the finished
 * reference, plus a trailing space so typing can continue.
 *
 * Backticked rather than bare: `path:line` is the convention this codebase
 * already uses for clickable references, and backticks stop Markdown from
 * mangling underscores when the text is echoed back.
 */
export function applyMention(
  text: string,
  start: number,
  caret: number,
  path: string,
  lines?: LineSpec,
): { text: string; caret: number } {
  const inserted = `${formatMention(path, lines)} `;
  const next = text.slice(0, start) + inserted + text.slice(caret);
  return { text: next, caret: start + inserted.length };
}
