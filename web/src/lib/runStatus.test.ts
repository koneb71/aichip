import { describe, expect, it } from "vitest";
import { isWorking, needsYou, statusLabel, stopReason } from "./runStatus";

describe("stopReason", () => {
  it("explains a failure in red", () => {
    const r = stopReason("failed", "API Error: Unable to connect to API (ConnectionRefused)");
    expect(r).toEqual({
      text: "API Error: Unable to connect to API (ConnectionRefused)",
      tone: "danger",
    });
  });

  it("treats a run that was stopped as an explanation, not a fault", () => {
    // What the attention timeout writes. Nothing crashed — aichip chose to
    // stop rather than tell the engine a person had refused.
    const r = stopReason(
      "canceled",
      "nobody answered the request to allow Bash after 24h; aichip stopped the run rather than telling it you had refused",
    );
    expect(r?.tone).toBe("amber");
  });

  it("does not paint a healthy parked run red", () => {
    // The gate writes this *while the run is alive and waiting on you*. This
    // is the case that makes the pair, rather than the column, the input.
    const r = stopReason("waiting_permission", "waiting for you to allow Bash");
    expect(r?.tone).toBe("note");
  });

  it("still lets a finished run say what went wrong along the way", () => {
    // A real org run: completed, and two of its assignments were dropped. On
    // the board that is indistinguishable from a clean win unless it is said.
    const r = stopReason("completed", "2 assignments were dropped after failing");
    expect(r).toEqual({ text: "2 assignments were dropped after failing", tone: "note" });
  });

  it("says nothing when there is no reason, whatever the status", () => {
    for (const s of ["failed", "canceled", "running", "completed", null, undefined]) {
      expect(stopReason(s, null)).toBeNull();
      expect(stopReason(s, "   ")).toBeNull();
    }
  });
});

describe("the two parked states are told apart", () => {
  it("needsYou covers both, and neither animates", () => {
    for (const s of ["awaiting_approval", "waiting_permission"]) {
      expect(needsYou(s)).toBe(true);
      expect(isWorking(s)).toBe(false);
    }
  });

  it("labels a tool prompt and an approval differently", () => {
    // The board called both "approval" and rendered one of them as idle.
    expect(statusLabel("waiting_permission")).toBe("needs your answer");
    expect(statusLabel("awaiting_approval")).toBe("needs your approval");
  });
});
