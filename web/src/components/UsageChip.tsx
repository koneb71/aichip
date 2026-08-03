import { useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, PlanLimit } from "../lib/api";
import { resetIn, windowPhrase } from "../lib/usage";

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
 *
 * It links to the usage panel on Activity, which is the same facts plus the
 * history: this says "not now", that says "how often is it not now". Labels
 * and the countdown come from `lib/usage` so the two cannot word the same
 * limit differently.
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
        <Link
          key={`${l.engine}-${l.limitType}`}
          to="/activity"
          className={`block rounded-lg px-2 py-1.5 text-[11px] leading-snug transition-opacity hover:opacity-80 ${
            l.status === "blocked"
              ? "bg-red-50 text-danger"
              : "bg-amber-50 text-amber-900"
          }`}
        >
          <span className="font-semibold">
            {l.status === "blocked" ? "Out of " : "Nearly out of "}
            {windowPhrase(l.limitType)}
          </span>
          {l.resetsAt && <> · back {resetIn(l.resetsAt, Date.now())}</>}
          {l.usingOverage && <> · on paid overage</>}
        </Link>
      ))}
    </div>
  );
}
