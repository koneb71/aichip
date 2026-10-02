import { describe, expect, it } from "vitest";
import { BOX, layoutTree, OrgNode, todayLine, wouldCycle } from "./orgChart";

const node = (id: string, reportsTo: string | null = null): OrgNode => ({
  id,
  name: id,
  title: null,
  icon: "bot",
  color: "#000",
  engine: null,
  modelTier: "medium",
  status: "active",
  reportsTo,
  liveCard: null,
  runsToday: 0,
  spendTodayUsd: 0,
  heartbeatSecs: null,
  lastHeartbeatAt: null,
});

describe("layoutTree", () => {
  it("centres a manager over its reports, one row per level", () => {
    const { positions, edges } = layoutTree([node("ceo"), node("a", "ceo"), node("b", "ceo")]);
    const ceo = positions.get("ceo")!;
    const a = positions.get("a")!;
    const b = positions.get("b")!;
    expect(a.y).toBe(b.y);
    expect(a.y).toBeGreaterThan(ceo.y);
    expect(ceo.x).toBe((a.x + b.x) / 2);
    expect(b.x - a.x).toBe(BOX.w + BOX.gapX);
    expect(edges).toEqual([
      { from: "ceo", to: "a" },
      { from: "ceo", to: "b" },
    ]);
  });

  it("never overlaps two boxes on a row", () => {
    const nodes = [node("r"), node("a", "r"), node("b", "r"), node("a1", "a"), node("a2", "a"), node("b1", "b"), node("solo")];
    const { positions } = layoutTree(nodes);
    const rows = new Map<number, number[]>();
    for (const p of positions.values()) rows.set(p.y, [...(rows.get(p.y) ?? []), p.x]);
    for (const xs of rows.values()) {
      const sorted = [...xs].sort((x, y) => x - y);
      for (let i = 1; i < sorted.length; i++) expect(sorted[i] - sorted[i - 1]).toBeGreaterThanOrEqual(BOX.w);
    }
    expect(positions.size).toBe(nodes.length);
  });

  it("puts agents not on the chart yet in a grid beneath the trees", () => {
    const loners = ["l1", "l2", "l3", "l4", "l5"].map((id) => node(id));
    const { positions } = layoutTree([node("ceo"), node("a", "ceo"), ...loners]);
    const treeBottom = positions.get("a")!.y;
    for (const l of loners) expect(positions.get(l.id)!.y).toBeGreaterThan(treeBottom);
    const rows = new Set(loners.map((l) => positions.get(l.id)!.y));
    expect(rows.size).toBe(2);
  });

  it("draws someone whose manager is missing as a root, and survives a loop in bad data", () => {
    const { positions } = layoutTree([node("x", "gone"), node("p", "q"), node("q", "p")]);
    expect(positions.get("x")!.y).toBe(0);
    expect(positions.size).toBe(3);
  });
});

describe("wouldCycle", () => {
  const nodes = [node("ceo"), node("lead", "ceo"), node("dev", "lead")];
  it("refuses moving a manager under its own report", () => {
    expect(wouldCycle(nodes, "ceo", "dev")).toBe(true);
    expect(wouldCycle(nodes, "lead", "lead")).toBe(true);
    expect(wouldCycle(nodes, "dev", "ceo")).toBe(false);
  });
});

describe("todayLine", () => {
  it("says nothing for an idle agent", () => {
    expect(todayLine({ runsToday: 0, spendTodayUsd: 0 })).toBeNull();
    expect(todayLine({ runsToday: 1, spendTodayUsd: 0 })).toBe("1 run today");
    expect(todayLine({ runsToday: 3, spendTodayUsd: 0.4 })).toBe("3 runs · $0.40 today");
  });
});
