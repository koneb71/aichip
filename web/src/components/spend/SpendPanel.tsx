import { useEffect, useState } from "react";
import { api, Spend, SpendDimension, SpendSlice } from "../../lib/api";
import { cacheHitLabel, compactTokens, patternLabel, sliceHitRate } from "../../lib/spend";
import { useWorkspace } from "../../lib/workspace";

/**
 * Where the tokens went.
 *
 * The card above this one says what the window cost. This says why, which is
 * the only version of the question you can act on: the dearest line here is
 * usually a *pattern* rather than a project — a bake-off is several runs on
 * one brief, a debate team is several attempts plus a judge, a plan-first card
 * is two passes. None of that is visible in a per-run figure.
 *
 * Fetched on demand rather than folded into the activity poll, which runs
 * every few seconds; this is six grouped aggregates over the run history and
 * has no business being asked at that rate.
 */
const DIMENSIONS: { id: SpendDimension; label: string; note?: string }[] = [
  { id: "pattern", label: "By feature", note: "Which part of aichip spent it" },
  { id: "project", label: "By project" },
  { id: "tier", label: "By tier", note: "The tier a run actually used" },
  { id: "model", label: "By model" },
  { id: "engine", label: "By engine" },
];

export function SpendPanel() {
  const { active } = useWorkspace();
  const [data, setData] = useState<Spend | null>(null);
  const [dim, setDim] = useState<SpendDimension>("pattern");
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    setFailed(false);
    api
      .spend(active?.id, 30)
      .then(setData)
      .catch(() => setFailed(true));
  }, [active?.id]);

  if (failed) {
    // A server older than this page has no /api/spend. Say so plainly rather
    // than rendering an empty breakdown that reads as "you spent nothing".
    return (
      <Card>
        <Heading>Breakdown</Heading>
        <p className="mt-2 text-xs text-ink-dim">
          This server doesn't report a spend breakdown yet.
        </p>
      </Card>
    );
  }
  if (!data) {
    return (
      <Card>
        <Heading>Breakdown</Heading>
        <p className="mt-2 text-xs text-ink-dim">Adding it up…</p>
      </Card>
    );
  }

  const slices = data.breakdowns[dim] ?? [];
  const { totals } = data;

  return (
    <Card>
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <Heading>Breakdown · last {data.days} days</Heading>
        <div className="flex flex-wrap gap-1">
          {DIMENSIONS.map((d) => (
            <button
              key={d.id}
              onClick={() => setDim(d.id)}
              className={`rounded-lg px-2 py-1 text-[11px] transition ${
                dim === d.id
                  ? "bg-accent text-white"
                  : "bg-panel-2 text-ink-dim hover:text-ink"
              }`}
            >
              {d.label}
            </button>
          ))}
        </div>
      </div>

      <div className="mt-4 flex flex-wrap gap-6">
        <Stat
          label="Served from cache"
          value={cacheHitLabel(data.cacheHitRate)}
          note="Cached input costs a fraction of fresh input"
        />
        <Stat label="Input" value={compactTokens(totals.inputTokens)} note="tokens" />
        <Stat label="Output" value={compactTokens(totals.outputTokens)} note="tokens" />
        <Stat label="Cached" value={compactTokens(totals.cacheReadTokens)} note="tokens" />
      </div>

      {/* State the gaps rather than letting a total look complete. */}
      {(totals.unpricedRuns > 0 || totals.provisionalRuns > 0) && (
        <p className="mt-3 text-[11px] leading-relaxed text-ink-dim/80">
          {totals.unpricedRuns > 0 && (
            <>
              {totals.unpricedRuns} run{totals.unpricedRuns === 1 ? "" : "s"} spent tokens
              without a reported price, so their cost is in no total here.{" "}
            </>
          )}
          {totals.provisionalRuns > 0 && (
            <>
              {totals.provisionalRuns} ended without a final tally — those token counts
              are the last figures reported, not a settled total.
            </>
          )}
        </p>
      )}

      <div className="mt-4 flex flex-col gap-2">
        {slices.length === 0 && (
          <p className="text-xs text-ink-dim">Nothing recorded in this window.</p>
        )}
        {slices.map((s) => (
          <Row key={s.key} slice={s} top={Math.max(...slices.map((x) => x.costUsd), 0.0001)} dim={dim} />
        ))}
      </div>

      <p className="mt-4 text-[11px] leading-relaxed text-ink-dim/80">
        Costs are what each CLI reported as it worked — aichip asks nothing and prices
        nothing itself.
      </p>
    </Card>
  );
}

function Row({ slice, top, dim }: { slice: SpendSlice; top: number; dim: SpendDimension }) {
  const hit = sliceHitRate(slice);
  return (
    <div className="flex items-center gap-3">
      <div className="w-32 shrink-0 truncate text-sm" title={slice.key}>
        {dim === "pattern" ? patternLabel(slice.key) : slice.key}
      </div>
      <div className="h-2 min-w-0 flex-1 overflow-hidden rounded-full bg-panel-2">
        <div
          style={{ width: `${(slice.costUsd / top) * 100}%` }}
          className="h-full rounded-full bg-accent transition-[width] duration-500 ease-out"
        />
      </div>
      <div
        className="w-14 shrink-0 text-right text-[11px] tabular-nums text-ink-dim"
        title="Share of this row's tokens served from cache"
      >
        {cacheHitLabel(hit)}
      </div>
      <div className="w-12 shrink-0 text-right text-[11px] tabular-nums text-ink-dim">
        {slice.runs} run{slice.runs === 1 ? "" : "s"}
      </div>
      <div className="w-16 shrink-0 text-right text-xs tabular-nums text-ink-dim">
        ${slice.costUsd.toFixed(2)}
      </div>
    </div>
  );
}

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="flex flex-col">
      <span className="text-[11px] uppercase tracking-wider text-ink-dim">{label}</span>
      <span className="text-xl font-bold tabular-nums">{value}</span>
      {note && <span className="text-[11px] text-ink-dim/80">{note}</span>}
    </div>
  );
}

function Card({ children }: { children: React.ReactNode }) {
  return (
    <div className="card-shadow mt-4 rounded-xl border border-line bg-panel p-5">{children}</div>
  );
}

function Heading({ children }: { children: React.ReactNode }) {
  return (
    <div className="text-xs font-semibold uppercase tracking-wider text-ink-dim">{children}</div>
  );
}
