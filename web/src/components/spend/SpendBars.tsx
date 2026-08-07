/** One day's spend, as the activity endpoint reports it. */
export type SpendDayPoint = { day: string; cost: number; runs: number };

/**
 * What the last fortnight cost, a bar per day.
 *
 * Shared by Activity, which gives it room, and Home, which shows a shorter
 * window at a smaller height. Parameterised rather than copied, because the one
 * thing here worth getting right is easy to lose in a copy: the axis.
 */
export function SpendBars({
  daily,
  days: windowDays = 14,
  height = 96,
  labels = true,
}: {
  daily: SpendDayPoint[];
  days?: number;
  /** Pixel height of the tallest bar. */
  height?: number;
  labels?: boolean;
}) {
  // The API only returns days that had runs. Plotting those alone turns two
  // busy days into two half-width bars that read as a single line and hides
  // the fact that nothing happened in between — so the axis is always the
  // full window, quiet days included.
  const byDay = new Map(daily.map((d) => [d.day.slice(0, 10), d]));
  const days = Array.from({ length: windowDays }, (_, i) => {
    const date = new Date();
    date.setDate(date.getDate() - (windowDays - 1 - i));
    const key = date.toISOString().slice(0, 10);
    return byDay.get(key) ?? { day: key, cost: 0, runs: 0 };
  });
  const top = Math.max(...days.map((d) => d.cost), 0.0001);

  return (
    <div>
      <div className="flex items-end gap-1.5" style={{ height }}>
        {days.map((d) => (
          <div key={d.day} className="group relative flex-1">
            {/* A CSS transition rather than a motion value. A JS-driven
                animation writes the *current* frame to the element, and
                rAF is suspended in a background tab — so the bars freeze
                part-grown and the chart reads as wrong data rather than as
                an unfinished animation. With a transition the final height
                is in the style immediately and only the approach animates. */}
            <div
              style={{ height: d.cost > 0 ? Math.max(3, (d.cost / top) * height) : 2 }}
              className={`w-full rounded-t transition-[height] duration-500 ease-out ${
                d.cost > 0 ? "bg-accent/70 group-hover:bg-accent" : "bg-line"
              }`}
            />
            <div className="pointer-events-none absolute bottom-full left-1/2 z-10 mb-1 hidden -translate-x-1/2 whitespace-nowrap rounded bg-ink px-2 py-1 text-[11px] text-white group-hover:block">
              {formatDay(d.day)} · ${d.cost.toFixed(2)} · {d.runs} run
              {d.runs === 1 ? "" : "s"}
            </div>
          </div>
        ))}
      </div>
      {labels && (
        <div className="mt-1.5 flex justify-between text-[11px] text-ink-dim">
          <span>{formatDay(days[0].day)}</span>
          <span>today</span>
        </div>
      )}
    </div>
  );
}

function formatDay(day: string): string {
  return new Date(day).toLocaleDateString(undefined, { month: "short", day: "numeric" });
}
