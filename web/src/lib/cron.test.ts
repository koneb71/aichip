import { describe, expect, it } from "vitest";
import { compile, describeCron, recognize, relative } from "./cron";

describe("the schedule builder round-trips", () => {
  // The property that matters: anything `compile` writes, `recognize` reads
  // back as the same fields. Break it and a saved schedule reopens in the
  // Custom tab showing raw cron — the editor forgetting what it just wrote.
  const cases: Array<[Parameters<typeof compile>, string]> = [
    [["daily", "09:00", 1, 1, ""], "0 9 * * *"],
    [["weekdays", "18:30", 1, 1, ""], "30 18 * * 1-5"],
    [["weekly", "07:15", 3, 1, ""], "15 7 * * 3"],
    [["monthly", "23:45", 1, 12, ""], "45 23 12 * *"],
    [["hourly", "09:20", 1, 1, ""], "20 * * * *"],
  ];

  for (const [args, expr] of cases) {
    it(`${args[0]} compiles to ${expr} and back`, () => {
      expect(compile(...args)).toBe(expr);
      const back = recognize(expr);
      expect(back.preset).toBe(args[0]);
      if (args[0] !== "hourly") expect(back.time).toBe(args[1]);
      if (args[0] === "weekly") expect(back.weekday).toBe(args[2]);
      if (args[0] === "monthly") expect(back.monthday).toBe(args[3]);
    });
  }
});

describe("recognize", () => {
  it("falls back to custom rather than guessing", () => {
    // A step expression is not a shape the builder writes. Claiming it is
    // "every day at 09:00" would be a confident lie about when work runs.
    expect(recognize("0 9 */2 * *").preset).toBe("custom");
    expect(recognize("not cron at all").preset).toBe("custom");
    expect(describeCron("0 9 */2 * *")).toBe("0 9 */2 * *");
  });

  it("pads the hour so the time input accepts it", () => {
    // <input type="time"> ignores "7:15" — the field would silently blank.
    expect(recognize("15 7 * * 3").time).toBe("07:15");
  });
});

describe("describeCron", () => {
  it("says the schedule in words", () => {
    expect(describeCron("0 9 * * *")).toBe("every day at 09:00");
    expect(describeCron("30 18 * * 1-5")).toBe("weekdays at 18:30");
    expect(describeCron("15 7 * * 3")).toBe("Wednesdays at 07:15");
    expect(describeCron("0 * * * *")).toBe("every hour");
  });
});

describe("relative", () => {
  const now = Date.parse("2026-08-17T12:00:00Z");
  const at = (iso: string) => relative(iso, now);

  it("reads forwards and backwards", () => {
    expect(at("2026-08-17T16:00:00Z")).toBe("in 4 h");
    expect(at("2026-08-17T11:40:00Z")).toBe("20 min ago");
  });

  it("does not say 0 min", () => {
    expect(at("2026-08-17T12:00:10Z")).toBe("in under a minute");
  });

  it("switches to days rather than counting 60 hours", () => {
    expect(at("2026-08-20T12:00:00Z")).toBe("in 3 days");
  });
});
