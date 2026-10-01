import { describe, expect, it } from "vitest";
import { parseMergeRefusal } from "./mergeRefusal";

describe("parseMergeRefusal", () => {
  it("reads a typed refusal", () => {
    expect(
      parseMergeRefusal(JSON.stringify({ kind: "conflict", error: "git … failed", files: ["a.rs"] })),
    ).toEqual({ kind: "conflict", error: "git … failed", files: ["a.rs"] });
  });
  it("treats plain text as untyped, not as an error", () => {
    expect(parseMergeRefusal("an agent is still working on this card")).toBeNull();
  });
  it("ignores JSON that is not a refusal", () => {
    expect(parseMergeRefusal(JSON.stringify({ kind: "other", error: "x" }))).toBeNull();
    expect(parseMergeRefusal("[]")).toBeNull();
  });
});
