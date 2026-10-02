import { describe, expect, it } from "vitest";
import { estimateLine, parseForecastAsk, soonestBurnout } from "./forecast";
import type { BudgetStanding } from "./api";

describe("parseForecastAsk", () => {
  it("reads the server's question", () => {
    const ask = parseForecastAsk(
      JSON.stringify({ kind: "forecast", message: "this could cost up to $3", policy: "Nightly", p90Usd: 3, medianUsd: 1, runs: 12, headroomUsd: 0.5, taskId: "t1" }),
    );
    expect(ask?.policy).toBe("Nightly");
    expect(ask?.taskId).toBe("t1");
  });
  it("is not fooled by other refusals", () => {
    expect(parseForecastAsk("blocked by schema — land that card first")).toBeNull();
    expect(parseForecastAsk(JSON.stringify({ kind: "conflict", error: "x" }))).toBeNull();
  });
});

describe("estimateLine", () => {
  it("says what the number stands on", () => {
    expect(estimateLine({ medianUsd: 0.8, p90Usd: 3, runs: 12, basis: "project" })).toBe(
      "~$0.80 median · up to $3.00, from 12 similar runs",
    );
    expect(estimateLine({ medianUsd: 0.031, p90Usd: 0.05, runs: 7, basis: "engine" })).toBe(
      "~$0.031 median · up to $0.050, from 7 runs on this engine",
    );
    expect(estimateLine(null)).toBeNull();
  });
});

describe("soonestBurnout", () => {
  const row = (name: string, at: string | null, state = "open"): BudgetStanding =>
    ({
      policy: { name, enabled: true },
      verdict: { state },
      forecast: at ? { cap: "usd", runsOutAt: at } : null,
    }) as unknown as BudgetStanding;
  it("picks the one that runs out first, ignoring spent ones", () => {
    expect(
      soonestBurnout([row("A", "2026-10-09T00:00:00Z"), row("B", "2026-10-06T00:00:00Z"), row("C", "2026-10-03T00:00:00Z", "exceeded")]),
    ).toEqual({ name: "B", at: "2026-10-06T00:00:00Z" });
    expect(soonestBurnout([row("A", null)])).toBeNull();
  });
});
