import { describe, expect, it } from "vitest";
import { adoptLegacyStorage, toolName } from "./brand";

/** A Storage over a Map, so the test needs no DOM. */
function memory(entries: Record<string, string>): Storage {
  const m = new Map(Object.entries(entries));
  return {
    get length() {
      return m.size;
    },
    key: (i: number) => [...m.keys()][i] ?? null,
    getItem: (k: string) => m.get(k) ?? null,
    setItem: (k: string, v: string) => void m.set(k, v),
    removeItem: (k: string) => void m.delete(k),
    clear: () => m.clear(),
  };
}

describe("adoptLegacyStorage", () => {
  it("moves every setting saved under the old name", () => {
    const s = memory({
      "aichip.workspace": "w1",
      "aichip.kb.draft.p1": "<p>draft</p>",
      "aichip:notify": "on",
      unrelated: "x",
    });
    expect(adoptLegacyStorage(s)).toBe(3);
    expect(s.getItem("eren.workspace")).toBe("w1");
    expect(s.getItem("eren.kb.draft.p1")).toBe("<p>draft</p>");
    expect(s.getItem("eren:notify")).toBe("on");
    expect(s.getItem("aichip.workspace")).toBeNull();
    expect(s.getItem("unrelated")).toBe("x");
  });

  it("keeps a value already saved under the new name, and runs once", () => {
    const s = memory({ "aichip.theme": "dark", "eren.theme": "light" });
    expect(adoptLegacyStorage(s)).toBe(0);
    expect(s.getItem("eren.theme")).toBe("light");
    expect(s.getItem("aichip.theme")).toBeNull();
    expect(adoptLegacyStorage(s)).toBe(0);
  });

  it("does not touch a key that only starts with the same letters", () => {
    const s = memory({ aichipper: "x" });
    expect(adoptLegacyStorage(s)).toBe(0);
    expect(s.getItem("aichipper")).toBe("x");
  });
});

describe("toolName", () => {
  it("shows a tool from an old transcript under its current name", () => {
    expect(toolName("mcp__aichip__create_task")).toBe("mcp__eren__create_task");
    expect(toolName("mcp__aichip")).toBe("mcp__eren");
    expect(toolName("mcp__eren__list_tasks")).toBe("mcp__eren__list_tasks");
    expect(toolName("mcp__aichipper__x")).toBe("mcp__aichipper__x");
    expect(toolName("Read")).toBe("Read");
  });
});
