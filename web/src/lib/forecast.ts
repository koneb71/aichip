import type { BudgetStanding, CostEstimate } from "./api";

/** A start the server wants confirmed: similar runs say it could overrun a budget. */
export interface ForecastAsk {
  kind: "forecast";
  message: string;
  policy: string;
  p90Usd: number;
  medianUsd: number;
  runs: number;
  headroomUsd: number;
  /** Set when the card was just created and sits in the backlog unstarted. */
  taskId?: string;
}

/** Read a start 409. Anything that is not the forecast question is not one. */
export function parseForecastAsk(text: string): ForecastAsk | null {
  try {
    const v = JSON.parse(text.replace(/^Error:\s*/, ""));
    if (v && v.kind === "forecast" && typeof v.message === "string") return v as ForecastAsk;
  } catch {
    // plain text: some other refusal
  }
  return null;
}

const usd = (n: number) => (n < 0.1 ? `$${n.toFixed(3)}` : `$${n.toFixed(2)}`);

/** "~$0.80 median · up to $3.00, from 12 similar runs" — with how far "similar" had to stretch. */
export function estimateLine(e: CostEstimate | null | undefined): string | null {
  if (!e) return null;
  const like = e.basis === "project" ? "similar runs" : e.basis === "tier" ? "runs at this tier" : "runs on this engine";
  return `~${usd(e.medianUsd)} median · up to ${usd(e.p90Usd)}, from ${e.runs} ${like}`;
}

/** The budget that runs out soonest at this rate, if any does before it resets. */
export function soonestBurnout(rows: BudgetStanding[]): { name: string; at: string } | null {
  let best: { name: string; at: string } | null = null;
  for (const r of rows) {
    const at = r.forecast?.runsOutAt;
    if (!at || !r.policy.enabled || r.verdict.state === "exceeded") continue;
    if (!best || at < best.at) best = { name: r.policy.name, at };
  }
  return best;
}
