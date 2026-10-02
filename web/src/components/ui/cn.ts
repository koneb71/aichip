/** Join class names, skipping the falsy ones. Small enough not to need a package. */
export function cn(...parts: Array<string | false | null | undefined>): string {
  return parts.filter(Boolean).join(" ");
}
