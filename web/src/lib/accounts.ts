/**
 * The account rules, mirrored from `eren_core::users` so a form can say what
 * is wrong before it is sent. The server decides; these only explain sooner.
 */

export const MIN_PASSWORD = 10;
export const MAX_PASSWORD = 256;

/** What a username becomes when stored: trimmed and lowercased. */
export function normalizeUsername(raw: string): string {
  return raw.trim().toLowerCase();
}

/** Why a username would be refused, or null. */
export function usernameProblem(raw: string): string | null {
  const name = normalizeUsername(raw);
  if (name.length < 3 || name.length > 32) return "A username is 3 to 32 characters.";
  if (!/^[a-z0-9._-]+$/.test(name)) return "A username uses letters, digits and . _ - only.";
  return null;
}

/** Why a new password would be refused, or null. `again` is the confirmation. */
export function passwordProblem(password: string, again?: string): string | null {
  const len = [...password].length;
  if (len < MIN_PASSWORD) return `A password is at least ${MIN_PASSWORD} characters.`;
  if (len > MAX_PASSWORD) return `A password is at most ${MAX_PASSWORD} characters.`;
  if (again !== undefined && again !== password) return "The two passwords differ.";
  return null;
}

/** The event a 401 from the API raises, so the sign-in screen comes back. */
export const SIGNED_OUT_EVENT = "eren:signed-out";
