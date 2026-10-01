/** Why a merge was refused, as the server types it. */
export interface MergeRefusal {
  kind: "dirty" | "conflict" | "markers";
  error: string;
  files: string[];
}

/**
 * Read a merge 409. The body is JSON from servers that type the refusal and
 * plain text from older ones — and from refusals that are not about the
 * merge itself, like a run still working — so plain text is not an error.
 */
export function parseMergeRefusal(text: string): MergeRefusal | null {
  try {
    const v = JSON.parse(text);
    if (v && typeof v.error === "string" && ["dirty", "conflict", "markers"].includes(v.kind)) {
      return { kind: v.kind, error: v.error, files: Array.isArray(v.files) ? v.files : [] };
    }
  } catch {
    // plain text
  }
  return null;
}
