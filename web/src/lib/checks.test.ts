import { describe, expect, it } from "vitest";
import { checksActive, checksChip, resultVerdict } from "./checks";

const result = { name: "t", command: "t", ms: 1, outputTail: "" };

describe("checksChip", () => {
  it("counts what passed", () => {
    expect(checksChip({ status: "passed", passed: 3, total: 3 })?.label).toBe("✓ 3/3");
    expect(checksChip({ status: "failed", passed: 1, total: 3 })).toMatchObject({ label: "✗ 1/3", tone: "bad" });
  });
  it("shows work in progress", () => {
    expect(checksChip({ status: "running", passed: 0, total: 2 })?.tone).toBe("busy");
  });
  it("draws nothing for no checks, or checks that newer work replaced", () => {
    expect(checksChip(null)).toBeNull();
    expect(checksChip({ status: "canceled", passed: 0, total: 2 })).toBeNull();
  });
});

describe("resultVerdict", () => {
  it("says how each command ended", () => {
    expect(resultVerdict({ ...result, exitCode: 0, timedOut: false })).toBe("passed");
    expect(resultVerdict({ ...result, exitCode: 101, timedOut: false })).toBe("exit 101");
    expect(resultVerdict({ ...result, exitCode: null, timedOut: true })).toBe("timed out");
    expect(resultVerdict({ ...result, exitCode: null, timedOut: false })).toBe("could not start");
  });
});

describe("checksActive", () => {
  it("is true only while they are still going", () => {
    expect(checksActive("queued")).toBe(true);
    expect(checksActive("running")).toBe(true);
    expect(checksActive("failed")).toBe(false);
  });
});
