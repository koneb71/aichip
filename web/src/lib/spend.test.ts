import { describe, expect, it } from "vitest";
import {
  cacheHitLabel,
  compactTokens,
  fillWindow,
  multipleLabel,
  patternLabel,
  sliceHitRate,
} from "./spend";
import { SpendDay, SpendSlice } from "./api";

const day = (d: string, cost: number): SpendDay => ({
  day: `${d}T00:00:00Z`,
  costUsd: cost,
  runs: 1,
  inputTokens: 100,
  outputTokens: 10,
  cacheReadTokens: 900,
});

const slice = (over: Partial<SpendSlice> = {}): SpendSlice => ({
  key: "task",
  costUsd: 1,
  runs: 1,
  inputTokens: 260,
  outputTokens: 59,
  cacheReadTokens: 12800,
  cacheCreationTokens: 800,
  medianUsd: 1,
  ...over,
});

describe("fillWindow", () => {
  it("puts the quiet days back", () => {
    // The whole point: two busy days a week apart must not end up adjacent,
    // reading as two runs in a row.
    const today = new Date("2026-08-02T12:00:00Z");
    const out = fillWindow([day("2026-08-02", 5), day("2026-07-27", 3)], 7, today);

    expect(out).toHaveLength(7);
    expect(out.map((d) => d.costUsd)).toEqual([3, 0, 0, 0, 0, 0, 5]);
  });

  it("ends on today, so the newest bar is the rightmost", () => {
    const today = new Date("2026-08-02T12:00:00Z");
    const out = fillWindow([], 3, today);
    expect(out.map((d) => d.day)).toEqual(["2026-07-31", "2026-08-01", "2026-08-02"]);
  });

  it("gives a filled day real zeroes rather than undefined counters", () => {
    const out = fillWindow([], 1, new Date("2026-08-02T12:00:00Z"));
    expect(out[0].runs).toBe(0);
    expect(out[0].cacheReadTokens).toBe(0);
  });
});

describe("cacheHitLabel", () => {
  it("distinguishes nothing-sent from nothing-cached", () => {
    // A fresh install must not read as "your cache never works".
    expect(cacheHitLabel(null)).toBe("—");
    expect(cacheHitLabel(0)).toBe("0%");
  });

  it("renders a rate as a whole percentage", () => {
    expect(cacheHitLabel(0.885)).toBe("89%");
    expect(cacheHitLabel(1)).toBe("100%");
  });
});

describe("sliceHitRate", () => {
  it("is the cached share of everything sent", () => {
    // 12800 of 13860.
    expect(sliceHitRate(slice())).toBeCloseTo(0.9235, 3);
  });

  it("is null when the slice sent nothing", () => {
    expect(
      sliceHitRate(slice({ inputTokens: 0, cacheReadTokens: 0, cacheCreationTokens: 0 })),
    ).toBeNull();
  });
});

describe("multipleLabel", () => {
  it("names a real difference", () => {
    expect(multipleLabel(2.6, 1)).toBe("2.6×");
  });

  it("stays quiet about noise and about missing baselines", () => {
    // A project with no plain runs yet has no baseline to compare against,
    // and a 4% difference is not a finding.
    expect(multipleLabel(1.04, 1)).toBeNull();
    expect(multipleLabel(2.6, 0)).toBeNull();
    expect(multipleLabel(2.6, null)).toBeNull();
    expect(multipleLabel(null, 1)).toBeNull();
  });
});

describe("compactTokens", () => {
  it("keeps small counts exact and shortens the rest", () => {
    expect(compactTokens(999)).toBe("999");
    expect(compactTokens(1_200)).toBe("1.2k");
    expect(compactTokens(22_600)).toBe("23k");
    expect(compactTokens(1_500_000)).toBe("1.5M");
  });
});

describe("patternLabel", () => {
  it("shows a pattern it has never heard of rather than dropping it", () => {
    // The dashboard is served from disk while the binary answering it may be
    // newer. An unknown pattern is still spend, and hiding it would make the
    // breakdown quietly fail to add up.
    expect(patternLabel("swarm")).toBe("swarm");
    expect(patternLabel("bakeoff")).toBe("Bake-offs");
  });
});
