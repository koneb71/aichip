import { describe, expect, it } from "vitest";
import { dueLine, flatten, Goal, parentChoices, progress } from "./goals";

const g = (id: string, parentId: string | null = null, position = 0): Goal => ({
  id,
  parentId,
  title: id,
  description: "",
  status: "active",
  targetDate: null,
  position,
  done: 0,
  total: 0,
  createdAt: "2026-01-01T00:00:00Z",
});

describe("flatten", () => {
  it("reads parent, then its children in order, then the next top goal", () => {
    const rows = flatten([g("b", null, 1), g("a", null, 0), g("a2", "a", 1), g("a1", "a", 0)]);
    expect(rows.map((r) => `${r.goal.id}:${r.depth}`)).toEqual(["a:0", "a1:1", "a2:1", "b:0"]);
    expect(rows[0].hasChildren).toBe(true);
  });
  it("keeps an orphan and survives a loop", () => {
    const rows = flatten([g("x", "gone"), g("p", "q"), g("q", "p")]);
    expect(rows.map((r) => r.goal.id).sort()).toEqual(["p", "q", "x"]);
  });
});

describe("parentChoices", () => {
  it("never offers a goal or anything under it as its own parent", () => {
    const all = [g("a"), g("b", "a"), g("c", "b"), g("d")];
    expect(parentChoices(all, "b").map((x) => x.id)).toEqual(["a", "d"]);
  });
});

describe("progress and due dates", () => {
  it("counts done over all, or nothing without cards", () => {
    expect(progress({ done: 0, total: 0 })).toBeNull();
    expect(progress({ done: 1, total: 3 })).toBe(33);
  });
  it("says how far off a target is", () => {
    const today = new Date(Date.UTC(2026, 9, 2));
    expect(dueLine(null, today)).toBeNull();
    expect(dueLine("2026-10-02", today)).toBe("due today");
    expect(dueLine("2026-10-14", today)).toBe("due in 12 days");
    expect(dueLine("2026-09-29", today)).toBe("3 days overdue");
  });
});
