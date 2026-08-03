import { useEffect, useState } from "react";
import { api, PlanLimit, UsageEvent, UsagePattern } from "../../lib/api";
import {
  isCurrent,
  resetIn,
  statusLabel,
  statusTone,
  transition,
  windowLabel,
} from "../../lib/usage";

/**
 * Where your Claude plan stands, and how often it stops you.
 *
 * The sidebar chip answers "can I start this now" and is silent when the
 * answer is yes. This is the place to look on purpose: every window aichip has
 * heard about, whether it is fine, when it turns over, and the history of when
 * it has pinched.
 *
 * ## Why there is no percentage bar
 *
 * Claude Code prints a *status* and a reset time, not a fraction of a quota.
 * aichip has no other source for it — reading the CLI's own config or calling
 * Anthropic are both things this project does not do, and one of them is a
 * credential it deliberately never holds. A progress bar here would be a
 * number invented to fill the space, which is worse than the honest shape:
 * these are the facts the CLI stated, with the time it stated them.
 *
 * The counts are days aichip *heard from* a limit, which is days you ran
 * something. They are never a percentage of "the time" — nothing is learned
 * on a day nothing runs, and a denominator that pretends otherwise would make
 * a quiet week look like a healthy one.
 */
export function UsagePanel() {
  const [limits, setLimits] = useState<PlanLimit[]>([]);
  const [events, setEvents] = useState<UsageEvent[]>([]);
  const [patterns, setPatterns] = useState<UsagePattern[]>([]);
  const [days, setDays] = useState(30);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    let alive = true;
    const load = () =>
      Promise.all([api.usage(), api.usageHistory()])
        .then(([now, past]) => {
          if (!alive) return;
          setLimits(now.limits);
          setEvents(past.events);
          setPatterns(past.patterns);
          setDays(past.days);
          setLoaded(true);
        })
        .catch(() => alive && setLoaded(true));
    load();
    // Slow on purpose: this only changes when a run reports, and no amount of
    // asking makes a run report sooner.
    const t = setInterval(load, 60_000);
    return () => {
      alive = false;
      clearInterval(t);
    };
  }, []);

  const now = Date.now();
  const live = limits.filter((l) => isCurrent(l.resetsAt, now));

  if (loaded && live.length === 0 && events.length === 0) {
    return (
      <p className="text-sm text-ink-dim">
        Nothing heard yet. Your CLI reports where your plan stands as it works,
        so this fills in after the first run — aichip asks Anthropic nothing.
      </p>
    );
  }

  return (
    <div className="space-y-4">
      <div className="grid gap-3 sm:grid-cols-2">
        {live.map((l) => (
          <LimitCard key={`${l.engine}-${l.limitType}`} limit={l} now={now} />
        ))}
      </div>

      {patterns.length > 0 && (
        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-ink-dim">
            Last {days} days
          </h3>
          <ul className="mt-3 space-y-2">
            {patterns.map((p) => (
              <li key={p.limitType} className="flex items-baseline justify-between gap-4 text-sm">
                <span className="font-medium">{windowLabel(p.limitType)}</span>
                <span className="text-right text-xs text-ink-dim">
                  {p.daysPinched === 0 ? (
                    <>never tight on {p.daysSeen} {p.daysSeen === 1 ? "day" : "days"} of running</>
                  ) : (
                    <>
                      tight on {p.daysPinched} of {p.daysSeen}{" "}
                      {p.daysSeen === 1 ? "day" : "days"} you ran anything
                      {p.timesBlocked > 0 && <> · stopped a run {p.timesBlocked}×</>}
                    </>
                  )}
                </span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {events.length > 0 && (
        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <h3 className="text-xs font-semibold uppercase tracking-wider text-ink-dim">
            What changed
          </h3>
          <ul className="mt-3 space-y-1.5">
            {events.slice(0, 12).map((e, i) => {
              const tone = statusTone(e.status);
              return (
                <li key={i} className="flex items-baseline gap-2 text-xs">
                  <span className={`mt-1.5 size-1.5 shrink-0 rounded-full ${tone.dot}`} />
                  <span className="font-medium">{windowLabel(e.limitType)}</span>
                  <span className="text-ink-dim">{transition(e.previous, e.status)}</span>
                  {e.usingOverage && <span className="text-amber-700">· paid overage</span>}
                  <span className="ml-auto shrink-0 text-ink-dim">
                    {new Date(e.observedAt).toLocaleString(undefined, {
                      month: "short",
                      day: "numeric",
                      hour: "numeric",
                      minute: "2-digit",
                    })}
                  </span>
                </li>
              );
            })}
          </ul>
        </div>
      )}

      <p className="text-xs leading-relaxed text-ink-dim">
        Your CLI prints this as it works and aichip keeps what it said — no
        credential, and nothing asked of Anthropic. So it is as fresh as your
        last run, and there is no percentage because the CLI does not report
        one.
      </p>
    </div>
  );
}

function LimitCard({ limit, now }: { limit: PlanLimit; now: number }) {
  const tone = statusTone(limit.status);
  const reset = resetIn(limit.resetsAt, now);
  return (
    <div className={`card-shadow rounded-xl border border-line p-4 ${tone.bg}`}>
      <div className="flex items-center gap-2">
        <span className={`size-2 rounded-full ${tone.dot}`} />
        <span className="text-sm font-semibold">{windowLabel(limit.limitType)}</span>
        <span className={`ml-auto text-xs font-medium ${tone.text}`}>
          {statusLabel(limit.status)}
        </span>
      </div>
      <div className="mt-2 text-xs text-ink-dim">
        {reset ? <>Turns over {reset}</> : <>No reset time reported</>}
        {limit.usingOverage && <> · on paid overage</>}
      </div>
      <div className="mt-1 text-[11px] text-ink-dim">
        as of {new Date(limit.updatedAt).toLocaleString(undefined, {
          month: "short",
          day: "numeric",
          hour: "numeric",
          minute: "2-digit",
        })}
      </div>
    </div>
  );
}
