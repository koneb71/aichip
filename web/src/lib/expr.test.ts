import { describe, expect, it } from "vitest";
import cases from "../../../crates/aichip-core/src/apps/expr_cases.json";
import { ExprError, fieldsUsed, parse, run, showIf, type Record_, type Val } from "./expr";

/**
 * The shared specification.
 *
 * This file and `crates/aichip-core/src/apps/expr.rs` read the same corpus, and
 * that is the only thing keeping two implementations of one language honest
 * with each other. A case added there fails on whichever side has not caught
 * up — which is the point, and why the corpus lives next to the Rust rather
 * than being copied here.
 */
type Case = {
  expr: string;
  record?: Record<string, Val>;
  now?: string;
  expect?: unknown;
  error?: boolean;
};

const DEFAULT_NOW = "2026-08-02T04:00:00Z";

describe("the shared expression corpus", () => {
  const corpus = cases as Case[];

  it("is worth sharing", () => {
    expect(corpus.length).toBeGreaterThan(30);
  });

  for (const c of corpus) {
    const label = c.expr === "" ? "(empty)" : c.expr;
    it(`${c.error ? "refuses" : "agrees on"} \`${label}\``, () => {
      const record = (c.record ?? {}) as Record_;
      const now = c.now ?? DEFAULT_NOW;

      if (c.error) {
        expect(() => run(c.expr, record, now)).toThrow();
        return;
      }
      const got = run(c.expr, record, now);
      // Numbers by value, not representation, for the same reason the Rust
      // side compares this way: the corpus has to be satisfiable in both.
      if (typeof got === "number" && typeof c.expect === "number") {
        expect(got).toBeCloseTo(c.expect, 9);
      } else {
        expect(got).toEqual(c.expect);
      }
    });
  }
});

describe("expression handling in the browser", () => {
  it("finds every field an expression reads, once each", () => {
    expect(fieldsUsed(parse("amount * qty + record.amount + len(note)")).sort()).toEqual([
      "amount",
      "note",
      "qty",
    ]);
  });

  it("says what was meant by a bare equals", () => {
    // What everyone writes the first time they mean ==.
    expect(() => parse("a = 1")).toThrow(ExprError);
    expect(() => parse("a = 1")).toThrow(/use == to compare/);
  });

  it("lists the real functions when one is misspelled", () => {
    expect(() => run("frobnicate(1)", {}, DEFAULT_NOW)).toThrow(/coalesce/);
  });

  it("bounds depth rather than blowing the stack", () => {
    const deep = "(".repeat(5000) + "1" + ")".repeat(5000);
    expect(() => parse(deep)).toThrow(/nested too deeply/);
  });

  describe("showIf", () => {
    it("shows a button when there is no condition", () => {
      expect(showIf(null, {}, DEFAULT_NOW)).toBe(true);
      expect(showIf(undefined, {}, DEFAULT_NOW)).toBe(true);
      expect(showIf("", {}, DEFAULT_NOW)).toBe(true);
    });

    it("hides one whose condition is false", () => {
      expect(showIf("category == ''", { category: "food" }, DEFAULT_NOW)).toBe(false);
      expect(showIf("category == ''", { category: "" }, DEFAULT_NOW)).toBe(true);
    });

    it("shows a button whose condition is broken, rather than hiding it", () => {
      // A button that vanishes because of a typo looks absent rather than
      // broken, and nobody ever finds out. Showing it surfaces the failure.
      expect(showIf("nonsense ===", {}, DEFAULT_NOW)).toBe(true);
      expect(showIf("frobnicate(1)", {}, DEFAULT_NOW)).toBe(true);
    });
  });
});
