import { describe, expect, it } from "vitest";
import { normalizeUsername, passwordProblem, usernameProblem } from "./accounts";

describe("account rules", () => {
  it("folds a username the way the server stores it", () => {
    expect(normalizeUsername("  Neiell ")).toBe("neiell");
    expect(usernameProblem("Neiell")).toBeNull();
    expect(usernameProblem("a.b_c-1")).toBeNull();
  });

  it("refuses what the server would", () => {
    for (const bad of ["ab", "has space", "émile", "x".repeat(33), "a/b"]) {
      expect(usernameProblem(bad), bad).not.toBeNull();
    }
  });

  it("holds passwords to a floor and asks for the same twice", () => {
    expect(passwordProblem("short")).not.toBeNull();
    expect(passwordProblem("long enough!")).toBeNull();
    expect(passwordProblem("long enough!", "long enough?")).toBe("The two passwords differ.");
    expect(passwordProblem("x".repeat(257))).not.toBeNull();
  });
});
