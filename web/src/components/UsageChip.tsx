import { useEffect, useState } from "react";
import { api, PlanLimit } from "../lib/api";

/**
 * How much of your plan is left, before it becomes a failed run.
 *
 * Silent while everything is fine — a permanent "all good" badge is noise, and
 * the sidebar footer is not a dashboard. It appears when a limit warns or
 * blocks, which is exactly when there is a decision to make: start the big
 * thing now, or wait for the window to turn over.
 *
 * The numbers come from the user's own CLI, which prints them as it works.
 * aichip asks Anthropic nothing and holds no credential — so this is as fresh
 * as the last run, and says so rather than implying it is live.
 */
export function UsageChip() {
  const [limits, setLimits] = useState<PlanLimit[]>([]);

  useEffect(() => {
    const load = () =>
      api
        .usage()
        .then((r) => setLimits(r.limits))
        .catch(() => {});
    load();
    // Slow on purpose: it only changes when a run reports, and a run reporting
    // is not something this page can make happen faster by asking.
    const t = setInterval(load, 60_000);
    return () => clearInterval(t);
  }, []);

  const notable = limits.filter((l) => l.status !== "allowed");
  if (notable.length === 0) return null;

  return (
    <div className="mb-2 space-y-1 px-2">
      {notable.map((l) => (
        <div
          key={`${l.engine}-${l.limitType}`}
          className={`rounded-lg px-2 py-1.5 text-[11px] leading-snug ${
            l.status === "blocked"
              ? "bg-red-50 text-danger"
              : "bg-amber-50 text-amber-900"
          }`}
        >
          <span className="font-semibold">
            {l.status === "blocked" ? "Out of " : "Nearly out of "}
            {window_(l.limitType)}
          </span>
          {l.resetsAt && <> · back {when(l.resetsAt)}</>}
          {l.usingOverage && <> · on paid overage</>}
        </div>
      ))}
    </div>
  );
}

/** `five_hour` is the CLI's word, not a person's. */
function window_(limitType: string): string {
  switch (limitType) {
    case "five_hour":
      return "this 5-hour window";
    case "seven_day":
      return "this week's usage";
    default:
      return limitType.replace(/_/g, " ");
  }
}

/** "in 2h", "Fri 14:00" — near things in relative terms, far ones by name. */
function when(iso: string): string {
  const then = new Date(iso);
  const mins = Math.round((then.getTime() - Date.now()) / 60_000);
  if (mins <= 0) return "now";
  if (mins < 60) return `in ${mins}m`;
  if (mins < 60 * 20) return `in ${Math.round(mins / 60)}h`;
  return then.toLocaleString(undefined, {
    weekday: "short",
    hour: "numeric",
    minute: "2-digit",
  });
}
