import { Link } from "react-router-dom";

/**
 * One number, large, with what it counts underneath.
 *
 * Declared twice before this existed — once on Home and once on Activity, with
 * only the mobile sizing differing, which meant the tiles on one page shrank on
 * a phone and the tiles on the other pushed the page sideways. This is the
 * version that behaves.
 *
 * `min-w-0` and the responsive type are load-bearing: a row of tiles whose
 * labels set a min-content floor is enough to make the page scroll
 * horizontally, and `max-w-*` on the container cannot claw that back.
 */
export function Stat({
  label,
  value,
  accent,
  to,
  hint,
}: {
  label: string;
  value: string;
  accent: string;
  /** Where this number is explained. Makes the tile a link. */
  to?: string;
  hint?: string;
}) {
  const body = (
    <div
      className={
        "card-shadow min-w-0 rounded-xl border border-line bg-panel p-3 sm:p-4 " +
        (to ? "transition-colors hover:border-ink-dim" : "")
      }
    >
      <div className="truncate text-xl font-bold sm:text-2xl" style={{ color: accent }}>
        {value}
      </div>
      <div className="mt-1 truncate text-[11px] text-ink-dim sm:text-xs">{label}</div>
      {hint && <div className="mt-0.5 truncate text-[10px] text-ink-dim/80">{hint}</div>}
    </div>
  );
  return to ? (
    <Link to={to} className="min-w-0">
      {body}
    </Link>
  ) : (
    body
  );
}
