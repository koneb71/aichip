/**
 * Whether a form still matches what it was opened with — the test that decides
 * if Escape or a stray click may close it (see the kit Dialog's `dismissible`).
 *
 * Compared by value, so typing a character and deleting it again is not a
 * change. Arrays compare in order, because reordering a team's members is an
 * edit; pass a set sorted when its order means nothing. `null`, `undefined`
 * and `""` are one value: each is "nothing entered", and fields spell it
 * differently before and after a person touches them.
 */
export function formChanged(saved: unknown, draft: unknown): boolean {
  return !same(saved, draft);
}

const empty = (v: unknown) => v === null || v === undefined || v === "";

const plain = (v: unknown): v is Record<string, unknown> =>
  typeof v === "object" && v !== null && !Array.isArray(v);

function same(a: unknown, b: unknown): boolean {
  if (empty(a) && empty(b)) return true;
  if (Array.isArray(a) && Array.isArray(b)) {
    return a.length === b.length && a.every((x, i) => same(x, b[i]));
  }
  if (plain(a) && plain(b)) {
    const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
    return [...keys].every((k) => same(a[k], b[k]));
  }
  return a === b;
}
