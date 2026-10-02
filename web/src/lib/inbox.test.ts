import { describe, expect, it } from "vitest";
import type { InboxItem, InboxKind } from "./api";
import { arrivals, GROUPS, grouped, KIND_LABEL } from "./inbox";

const item = (key: string, kind: InboxKind): InboxItem => ({
  key,
  kind,
  title: key,
  detail: null,
  projectId: null,
  projectName: null,
  taskId: null,
  runId: null,
  link: "/",
  createdAt: "2026-10-02T00:00:00Z",
  actions: [],
  read: false,
  snoozedUntil: null,
  options: [],
});

describe("inbox", () => {
  it("announces nothing on the first look, then only what is new", () => {
    const a = item("plan:1", "plan");
    const b = item("question:2", "question");
    expect(arrivals(null, [a, b])).toEqual([]);
    expect(arrivals(new Set(["plan:1"]), [a, b])).toEqual([b]);
  });

  it("puts every kind in exactly one group, and drops empty groups", () => {
    const kinds = Object.keys(KIND_LABEL) as InboxKind[];
    for (const k of kinds) {
      expect(GROUPS.filter((g) => g.kinds.includes(k)).length, k).toBe(1);
    }
    const g = grouped([item("decision:1", "decision"), item("permission:x", "permission")]);
    expect(g.map((x) => x.label)).toEqual(["Blocking a run", "Proposals"]);
  });
});
