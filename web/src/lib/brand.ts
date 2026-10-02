/**
 * The product's name, and the one it had before.
 *
 * The dashboard's half of `crates/eren-shared/src/brand.rs`: Eren was called
 * aichip, and a browser that used it then has settings saved under the old
 * name and transcripts whose tool calls carry it. Every old spelling the
 * dashboard reads lives here, so the rest of it never mentions one.
 */

export const NAME = "eren";
const LEGACY = "aichip";

/**
 * Move every setting saved under the old name to the new one, once.
 *
 * Generic over the key rather than a list of them, so a remembered workspace,
 * a draft of an article and the theme all come across without anyone having
 * to remember which keys existed. A value already saved under the new name
 * wins — it was written later. Returns how many keys moved.
 */
export function adoptLegacyStorage(storage: Storage): number {
  const legacy: string[] = [];
  for (let i = 0; i < storage.length; i++) {
    const key = storage.key(i);
    if (key && (key.startsWith(`${LEGACY}.`) || key.startsWith(`${LEGACY}:`))) legacy.push(key);
  }
  let moved = 0;
  for (const key of legacy) {
    const renamed = NAME + key.slice(LEGACY.length);
    const value = storage.getItem(key);
    if (value !== null && storage.getItem(renamed) === null) {
      storage.setItem(renamed, value);
      moved++;
    }
    storage.removeItem(key);
  }
  return moved;
}

/**
 * A tool name with the old server name rewritten to the new one, for showing
 * a transcript recorded before the rename the way a new one is shown.
 */
export function toolName(name: string): string {
  const old = `mcp__${LEGACY}`;
  if (name === old) return `mcp__${NAME}`;
  if (name.startsWith(`${old}__`)) return `mcp__${NAME}${name.slice(old.length)}`;
  return name;
}
