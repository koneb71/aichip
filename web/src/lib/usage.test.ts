import { describe, expect, it } from "vitest";
import {
  isCurrent,
  resetIn,
  statusLabel,
  transition,
  windowLabel,
  windowPhrase,
} from "./usage";

describe("windowLabel", () => {
  it("says what the window is in words a person uses", () => {
    expect(windowLabel("five_hour")).toBe("5-hour window");
    expect(windowLabel("seven_day")).toBe("Weekly usage");
  });

  it("has a mid-sentence form, because a heading is not a sentence", () => {
    // The chip says "nearly out of <this>". Lower-casing the heading gives
    // "nearly out of weekly usage", which is neither grammar.
    expect(windowPhrase("seven_day")).toBe("this week's usage");
    expect(windowPhrase("five_hour")).toBe("this 5-hour window");
  });

  it("falls back to the CLI's own word rather than hiding an unknown limit", () => {
    // A limit type we have never seen should still appear, legibly. Showing
    // nothing would mean a new kind of wall is invisible on the day it ships.
    expect(windowLabel("thirty_day")).toBe("thirty day");
  });
});

describe("resetIn", () => {
  const now = Date.UTC(2026, 7, 4, 12, 0, 0);
  const at = (mins: number) => new Date(now + mins * 60_000).toISOString();

  it("counts down in minutes, then hours", () => {
    expect(resetIn(at(25), now)).toBe("in 25m");
    expect(resetIn(at(180), now)).toBe("in 3h");
  });

  it("names the day once it is far enough away to be a plan, not a wait", () => {
    // Past 20 hours "in 34h" stops being useful — you want to know it is
    // Thursday afternoon.
    expect(resetIn(at(60 * 34), now)).toMatch(/\w{3}/);
  });

  it("does not show a negative countdown", () => {
    expect(resetIn(at(-10), now)).toBe("any moment");
  });

  it("has nothing to say when the CLI gave no reset time", () => {
    expect(resetIn(null, now)).toBeNull();
    expect(resetIn("not a date", now)).toBeNull();
  });
});

describe("isCurrent", () => {
  const now = Date.UTC(2026, 7, 4, 12, 0, 0);

  it("treats a window that has already refilled as stale", () => {
    // Otherwise last Tuesday's warning shows as though you were near the edge
    // right now.
    expect(isCurrent(new Date(now - 60_000).toISOString(), now)).toBe(false);
    expect(isCurrent(new Date(now + 60_000).toISOString(), now)).toBe(true);
  });

  it("keeps a limit with no reset time, since nothing says it expired", () => {
    expect(isCurrent(null, now)).toBe(true);
  });
});

describe("transition", () => {
  it("reads as the change it recorded", () => {
    expect(transition("warning", "blocked")).toBe("nearly out → out");
    expect(transition("blocked", "allowed")).toBe("out → fine");
  });

  it("does not invent a previous state for a limit's first sighting", () => {
    expect(transition(null, "allowed")).toBe("first seen · fine");
  });

  it("calls a same-status change what it is: the window turning over", () => {
    // Recorded because the reset time moved, not the status — which is the
    // most useful line in the log and would otherwise read as a no-op.
    expect(transition("allowed", "allowed")).toBe("window reset · fine");
  });
});

describe("statusLabel", () => {
  it("answers can-I-start-something, not what the CLI called it", () => {
    expect(statusLabel("allowed")).toBe("Fine");
    expect(statusLabel("warning")).toBe("Nearly out");
    expect(statusLabel("blocked")).toBe("Out");
  });
});
