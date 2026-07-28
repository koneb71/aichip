import { describe, expect, it } from "vitest";
import { applyMention, formatMention, mentionToken, parseLineSpec } from "./mention";

describe("mentionToken", () => {
  it("finds a token at the caret", () => {
    expect(mentionToken("see @src/li", 11)).toEqual({
      start: 4,
      query: "src/li",
      lineQuery: null,
    });
  });

  it("opens on a bare @ so the picker can show something immediately", () => {
    expect(mentionToken("@", 1)).toEqual({ start: 0, query: "", lineQuery: null });
  });

  it("ignores an @ that is not preceded by whitespace", () => {
    // Otherwise every email address would open a file picker.
    expect(mentionToken("mail me at a@b.com", 18)).toBeNull();
  });

  it("treats a newline as whitespace before the @", () => {
    expect(mentionToken("line one\n@api", 13)).toEqual({
      start: 9,
      query: "api",
      lineQuery: null,
    });
  });

  it("closes the token once whitespace is typed", () => {
    expect(mentionToken("@api.ts and then", 16)).toBeNull();
  });

  it("splits a line suffix off the path", () => {
    expect(mentionToken("@api.ts:10-25", 13)).toEqual({
      start: 0,
      query: "api.ts",
      lineQuery: "10-25",
    });
  });

  it("reports an empty lineQuery the moment ':' is typed", () => {
    // This is what flips the picker into line mode.
    expect(mentionToken("@api.ts:", 8)).toEqual({
      start: 0,
      query: "api.ts",
      lineQuery: "",
    });
  });

  it("uses the token the caret is in, not a later one", () => {
    const text = "@one and @two";
    expect(mentionToken(text, 4)).toEqual({ start: 0, query: "one", lineQuery: null });
  });

  it("returns null with no @ at all", () => {
    expect(mentionToken("nothing here", 12)).toBeNull();
  });
});

describe("parseLineSpec", () => {
  it("parses a single line", () => {
    expect(parseLineSpec("42")).toEqual({ start: 42 });
  });

  it("parses a range", () => {
    expect(parseLineSpec("10-25")).toEqual({ start: 10, end: 25 });
  });

  it("swaps a reversed range", () => {
    expect(parseLineSpec("25-10")).toEqual({ start: 10, end: 25 });
  });

  it("collapses a single-line range", () => {
    expect(parseLineSpec("7-7")).toEqual({ start: 7 });
  });

  it("rejects junk and non-positive lines", () => {
    // Line numbers are 1-based, so 0 is meaningless.
    for (const bad of ["", "  ", "0", "0-5", "-5", "abc", "1-", "1-2-3", "1.5"]) {
      expect(parseLineSpec(bad), `expected ${JSON.stringify(bad)} to be rejected`).toBeNull();
    }
  });
});

describe("formatMention / applyMention", () => {
  it("formats with and without lines", () => {
    expect(formatMention("web/src/lib/api.ts")).toBe("`web/src/lib/api.ts`");
    expect(formatMention("a.ts", { start: 42 })).toBe("`a.ts:42`");
    expect(formatMention("a.ts", { start: 10, end: 25 })).toBe("`a.ts:10-25`");
    // A degenerate range renders as a single line.
    expect(formatMention("a.ts", { start: 10, end: 10 })).toBe("`a.ts:10`");
  });

  it("replaces the token and leaves the caret after a trailing space", () => {
    const { text, caret } = applyMention("see @src/li", 4, 11, "src/lib/api.ts");
    expect(text).toBe("see `src/lib/api.ts` ");
    expect(caret).toBe(text.length);
  });

  it("keeps whatever followed the caret", () => {
    const { text, caret } = applyMention("see @ap rest", 4, 7, "api.ts");
    expect(text).toBe("see `api.ts`  rest");
    expect(text.slice(caret)).toBe(" rest");
  });

  it("carries a line range into the inserted reference", () => {
    const { text } = applyMention("@api", 0, 4, "web/api.ts", { start: 10, end: 25 });
    expect(text).toBe("`web/api.ts:10-25` ");
  });
});
