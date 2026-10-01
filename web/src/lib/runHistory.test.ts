import { describe, expect, it } from "vitest";
import { dollars, duration, tokensLine, triggerLabel } from "./runHistory";

const tokens = {
  inputTokens: 0,
  outputTokens: 0,
  cacheReadTokens: 0,
  cacheCreationTokens: 0,
  tokensProvisional: false,
};

describe("triggerLabel", () => {
  it("names every kind of follow-up", () => {
    expect(triggerLabel("review")).toBe("Fix from review");
    expect(triggerLabel("checks")).toBe("Fix failing checks");
    expect(triggerLabel("conflict")).toBe("Resolve conflicts");
  });
  it("says when a manual run planned first", () => {
    expect(triggerLabel("manual")).toBe("Run");
    expect(triggerLabel("manual", true)).toBe("Plan, then run");
  });
  it("falls back to the raw trigger, capitalised", () => {
    expect(triggerLabel("someday")).toBe("Someday");
  });
});

describe("duration", () => {
  it("formats each scale", () => {
    expect(duration(41)).toBe("41s");
    expect(duration(362)).toBe("6m 02s");
    expect(duration(3780)).toBe("1h 03m");
  });
  it("says nothing for a run that has not finished", () => {
    expect(duration(null)).toBe("—");
  });
});

describe("tokensLine", () => {
  it("is empty for a run that never spoke", () => {
    expect(tokensLine(tokens)).toBe("");
  });
  it("adds cache reads and writes together", () => {
    expect(
      tokensLine({ ...tokens, inputTokens: 12_800, outputTokens: 3_100, cacheReadTokens: 30_000, cacheCreationTokens: 10_000 }),
    ).toBe("13k in · 3.1k out · 40k cached");
  });
  it("marks an unconfirmed count as an estimate", () => {
    expect(tokensLine({ ...tokens, inputTokens: 900, outputTokens: 10, tokensProvisional: true })).toBe(
      "≈ 900 in · 10 out",
    );
  });
});

describe("dollars", () => {
  it("does not round a real cost to zero", () => {
    expect(dollars(0.004)).toBe("<$0.01");
    expect(dollars(1.844)).toBe("$1.84");
    expect(dollars(null)).toBe("—");
  });
});
