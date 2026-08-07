/**
 * Bytes as a person reads them.
 *
 * Shared because two surfaces now report disk — preview images and card
 * worktrees — and "828 MB" appearing next to "0.83 GB" for the same order of
 * magnitude reads as two different measurements rather than one.
 *
 * Decimal, not binary: this sits next to what `du -sh` and Finder say, and
 * matching them matters more than matching `ls -l`.
 */
export function size(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  return `${bytes} B`;
}
