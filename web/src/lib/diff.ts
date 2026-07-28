/**
 * Reading a unified diff well enough to point at a line of it.
 *
 * The diff view used to be a coloured blob of text — fine to read, useless to
 * refer to. To attach a review note to "the null check on line 42" you need
 * two things the raw text doesn't give you: which file each line belongs to,
 * and what its line number is in the *new* file (the one that exists now).
 *
 * Kept pure and separate so the line-number arithmetic — the part that is
 * quietly easy to get wrong — can be tested without rendering anything.
 */

export type DiffLineKind = "add" | "del" | "context" | "hunk" | "meta";

export interface DiffLine {
  text: string;
  kind: DiffLineKind;
  /** Path in the new file, or null before the first file header. */
  file: string | null;
  /** Line number in the new file. Null for deletions and metadata. */
  newLine: number | null;
  /** Which hunk this line belongs to; -1 before the first one. */
  hunk: number;
}

/** `@@ -12,7 +12,9 @@` → the new-file start line, or null if unparseable. */
function newStart(header: string): number | null {
  const match = /@@\s+-\d+(?:,\d+)?\s+\+(\d+)(?:,\d+)?\s+@@/.exec(header);
  return match ? parseInt(match[1], 10) : null;
}

export function annotateDiff(diff: string): DiffLine[] {
  const out: DiffLine[] = [];
  let file: string | null = null;
  let newLine = 0;
  let hunk = -1;

  for (const text of diff.split("\n")) {
    // `+++ b/path` names the new file. Checked before the `+` test below,
    // which would otherwise read the header as an added line.
    if (text.startsWith("+++ ")) {
      const path = text.slice(4).trim();
      file = path === "/dev/null" ? null : path.replace(/^b\//, "");
      out.push({ text, kind: "meta", file, newLine: null, hunk });
      continue;
    }
    if (text.startsWith("@@")) {
      hunk += 1;
      const start = newStart(text);
      if (start !== null) newLine = start;
      out.push({ text, kind: "hunk", file, newLine: null, hunk });
      continue;
    }
    if (text.startsWith("diff ") || text.startsWith("--- ") || text.startsWith("index ")) {
      out.push({ text, kind: "meta", file, newLine: null, hunk });
      continue;
    }
    if (text.startsWith("+")) {
      out.push({ text, kind: "add", file, newLine, hunk });
      newLine += 1;
      continue;
    }
    if (text.startsWith("-")) {
      // A deleted line has no number in the new file, and does not advance it.
      out.push({ text, kind: "del", file, newLine: null, hunk });
      continue;
    }
    // Context. `\ No newline at end of file` is metadata wearing a space.
    if (text.startsWith("\\")) {
      out.push({ text, kind: "meta", file, newLine: null, hunk });
      continue;
    }
    out.push({ text, kind: "context", file, newLine: newLine || null, hunk });
    if (newLine) newLine += 1;
  }
  return out;
}

/**
 * The text of one hunk, capped.
 *
 * Snapshotted into the comment because the fix run rewrites the diff: by the
 * time anyone reads the note back, "line 42" points somewhere else entirely.
 */
export function hunkText(lines: DiffLine[], hunk: number, maxLines = 40): string {
  const body = lines.filter((l) => l.hunk === hunk && l.kind !== "meta");
  return body
    .slice(0, maxLines)
    .map((l) => l.text)
    .join("\n");
}

/** Anything worth anchoring a comment to. Metadata isn't. */
export function isCommentable(line: DiffLine): boolean {
  return line.kind === "add" || line.kind === "del" || line.kind === "context";
}
