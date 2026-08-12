import type { StopTone } from "../../lib/runStatus";

/**
 * What a run said on its way out.
 *
 * Declared three times before this existed — in the workflow drawer, the org
 * run view and the bake-off list — byte-identical apart from a margin class,
 * and in none of them for a plain card, which is the most common thing in the
 * product. A card that burned $7 on `ConnectionRefused` looked exactly like a
 * card that had quietly gone idle.
 *
 * The tone comes from `stopReason`, never from the caller: `error_reason`
 * doubles as a live status line while a run is parked, so choosing the colour
 * at the call site is how a healthy run ends up in a red box.
 */
const TONE: Record<StopTone, string> = {
  danger: "bg-red-50 text-danger",
  amber: "bg-amber-50 text-amber-800",
  note: "bg-panel-2 text-ink-dim",
};

export function RunError({
  reason,
  tone = "danger",
  className = "",
  /** One line with the rest on hover — for a card, where height is shared. */
  compact = false,
}: {
  reason?: string | null;
  tone?: StopTone;
  className?: string;
  compact?: boolean;
}) {
  if (!reason?.trim()) return null;
  return (
    <div
      // `title` rather than a shortened string: CSS truncation keeps the whole
      // text in the DOM for the tooltip and for copy-paste, and a 4KB stack
      // trace cannot break the layout the way a `.slice()` on a 79-character
      // prefix of boilerplate can.
      title={compact ? reason : undefined}
      className={`rounded-lg px-2.5 py-1.5 text-[11px] leading-relaxed ${TONE[tone]} ${
        compact ? "truncate" : "whitespace-pre-wrap"
      } ${className}`}
    >
      {reason}
    </div>
  );
}
