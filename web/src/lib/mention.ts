/**
 * Parsing for `@`-mentions — project files, and agents from the library.
 *
 * Kept out of the component so the fiddly parts — where a token starts, what
 * counts as a line range — are unit-tested rather than only clickable, and so
 * that typing `@api.ts:10-25` by hand behaves exactly like picking it in the UI.
 *
 * The agent half of this file exists twice: `crates/aichip-core/src/runs/
 * mentions.rs` decides which agent a task actually binds to, and this side only
 * draws the chip. They read the same corpus — `mention_cases.json`, next to the
 * Rust — because a chip drawn for a mention that did not bind is worse than no
 * chip at all.
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
  return insert(text, start, caret, formatMention(path, lines));
}

/**
 * Replace the mention token with `@Name`.
 *
 * Bare rather than backticked, unlike a file. A file reference is a path being
 * quoted; an agent mention is a person being addressed, and it has to read that
 * way both in the message bubble and in the prompt the assistant is handed.
 */
export function applyAgentMention(
  text: string,
  start: number,
  caret: number,
  name: string,
): { text: string; caret: number } {
  return insert(text, start, caret, `@${name}`);
}

function insert(
  text: string,
  start: number,
  caret: number,
  body: string,
): { text: string; caret: number } {
  const inserted = `${body} `;
  return {
    text: text.slice(0, start) + inserted + text.slice(caret),
    caret: start + inserted.length,
  };
}

export interface AgentSpan {
  /** Index of the `@`. */
  start: number;
  /** One past the last character of the name. */
  end: number;
  /** The library's spelling, not what was typed. */
  name: string;
}

/** Letters, digits, `-` and `_` continue a word; a name has to run to the end
 *  of one. This is what stops an agent called "Front" claiming half of
 *  `@Frontend`.
 *
 *  Written with Unicode property escapes rather than `\w`, and the space test
 *  below with `\p{White_Space}` rather than `\s`, so that both match Rust's
 *  `char::is_alphanumeric` and `char::is_whitespace` exactly. `\s` does not:
 *  it counts U+FEFF, which Rust does not, and misses U+0085, which Rust
 *  counts — either one is a chip drawn for a mention that did not bind. */
const WORD = /[\p{Alphabetic}\p{N}\-_]/u;
const SPACE = /\p{White_Space}/u;

/**
 * Every `@agent` mention in `text`, in the order they appear.
 *
 * An `@` only counts at the start or after whitespace — the same rule the file
 * token uses, and what keeps an address out of it. Names are matched
 * longest-first so "Frontend" cannot swallow a mention of "Frontend Reviewer",
 * case-insensitively, and the *library's* spelling is what comes back.
 */
export function agentSpans(text: string, names: string[]): AgentSpan[] {
  const ordered = [...names].sort(
    (a, b) => [...b].length - [...a].length || (a < b ? -1 : a > b ? 1 : 0),
  );
  const out: AgentSpan[] = [];
  for (let i = 0; i < text.length; i++) {
    if (text[i] !== "@") continue;
    if (i > 0 && !SPACE.test(text[i - 1])) continue;
    const rest = text.slice(i + 1);
    for (const name of ordered) {
      const len = matchCaseless(rest, name);
      if (len === null) continue;
      out.push({ start: i, end: i + 1 + len, name });
      break;
    }
  }
  return out;
}

/** The distinct agents mentioned, first appearance first. */
export function agentMentions(text: string, names: string[]): string[] {
  const out: string[] = [];
  for (const span of agentSpans(text, names)) {
    if (!out.includes(span.name)) out.push(span.name);
  }
  return out;
}

/**
 * Does an agent called `name` answer to what has been typed so far?
 *
 * Both sides are reduced to letters and digits, so `@airbnbvibe` and
 * `@vibefrontend` both find "Airbnb-Vibe Frontend". The mention token stops at
 * whitespace — it has to, or a file mention could never end — so a picker that
 * only matched the literal prefix would be unable to find any agent whose name
 * has a space in it.
 */
export function agentMatches(query: string, name: string): boolean {
  const squash = (s: string) => s.toLowerCase().replace(/[^\p{Alphabetic}\p{N}]+/gu, "");
  return squash(name).includes(squash(query));
}

/** Length matched in `hay`, or null. Compares by code point so a multi-byte
 *  name cannot be split down the middle. */
function matchCaseless(hay: string, needle: string): number | null {
  if (!needle) return null;
  let at = 0;
  for (const want of needle) {
    const got = codePointAt(hay, at);
    if (got === null || got.toLowerCase() !== want.toLowerCase()) return null;
    at += got.length;
  }
  const next = codePointAt(hay, at);
  return next !== null && WORD.test(next) ? null : at;
}

function codePointAt(s: string, i: number): string | null {
  if (i >= s.length) return null;
  return String.fromCodePoint(s.codePointAt(i)!);
}
