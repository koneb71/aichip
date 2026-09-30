import { describe, expect, it } from "vitest";
import { backoff, parseFrame, SeqLedger } from "./ws";

describe("SeqLedger", () => {
  it("admits each seq once", () => {
    const l = new SeqLedger();
    expect(l.admit(0)).toBe(true);
    expect(l.admit(0)).toBe(false);
    expect(l.floor).toBe(0);
  });

  it("does not resume past a seq that has not arrived yet", () => {
    // Concurrent steps publish out of order: 2 lands before 1.
    const l = new SeqLedger();
    l.admit(0);
    l.admit(2);
    expect(l.floor).toBe(0);
    expect(l.admit(1)).toBe(true);
    expect(l.floor).toBe(2);
    // A replay from the floor re-sends nothing already shown.
    expect(l.admit(2)).toBe(false);
  });

  it("always passes ephemeral events through", () => {
    const l = new SeqLedger();
    expect(l.admit(-1)).toBe(true);
    expect(l.admit(-1)).toBe(true);
    expect(l.floor).toBe(-1);
  });
});

describe("parseFrame", () => {
  it("lifts step_id off a replay frame", () => {
    const e = parseFrame(
      JSON.stringify({ runId: "r", seq: 3, ts: "t", step_id: "s", event: { type: "text" } }),
    );
    expect(e).toMatchObject({ runId: "r", seq: 3, step_id: "s", type: "text" });
  });

  it("reads a flat live frame", () => {
    const e = parseFrame(JSON.stringify({ run_id: "r", seq: 4, ts: "t", type: "text" }));
    expect(e).toMatchObject({ runId: "r", seq: 4, type: "text" });
  });

  it("drops a malformed frame", () => {
    expect(parseFrame("{nope")).toBeNull();
  });
});

describe("backoff", () => {
  it("grows and caps", () => {
    expect(backoff(0)).toBe(500);
    expect(backoff(1)).toBe(1000);
    expect(backoff(20)).toBe(10_000);
  });
});
