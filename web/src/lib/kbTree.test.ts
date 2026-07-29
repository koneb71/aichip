import { describe, expect, it } from "vitest";
import {
  breadcrumbOf,
  depthOf,
  descendantsOf,
  heightOf,
  legalParents,
  nest,
  TreePage,
  visibleRows,
  wouldCycle,
} from "./kbTree";

const page = (id: string, parentId: string | null, position = 0): TreePage => ({
  id,
  parentId,
  title: id,
  icon: "",
  position,
  status: "published",
  origin: "human",
  childCount: 0,
  hasPending: false,
  writing: false,
});

/** a → b → c, plus a sibling d at the root. */
const tree: TreePage[] = [
  page("a", null, 1),
  page("b", "a", 1),
  page("c", "b", 1),
  page("d", null, 2),
];

describe("nest", () => {
  it("builds the hierarchy the server's flat list describes", () => {
    const roots = nest(tree);
    expect(roots.map((r) => r.id)).toEqual(["a", "d"]);
    expect(roots[0].children[0].id).toBe("b");
    expect(roots[0].children[0].children[0].id).toBe("c");
  });

  it("assigns depth by walking, so indentation matches structure", () => {
    const roots = nest(tree);
    expect(roots[0].depth).toBe(0);
    expect(roots[0].children[0].depth).toBe(1);
    expect(roots[0].children[0].children[0].depth).toBe(2);
  });

  /** A child whose parent is in another space would otherwise vanish with
   *  nothing to say where it went. */
  it("lifts an orphan to the root rather than dropping it", () => {
    const roots = nest([page("x", "missing-parent")]);
    expect(roots.map((r) => r.id)).toEqual(["x"]);
  });

  /** The server refuses to create one, but a render that hangs on corrupt
   *  data takes the whole page down. */
  it("does not recurse forever on a cycle", () => {
    const cyclic = [page("p", "q"), page("q", "p")];
    expect(() => nest(cyclic)).not.toThrow();
  });

  it("preserves the server's ordering", () => {
    const roots = nest([page("second", null, 2), page("first", null, 1)]);
    // The server orders; the client must not resort and disagree with it.
    expect(roots.map((r) => r.id)).toEqual(["second", "first"]);
  });
});

describe("visibleRows", () => {
  it("shows only what is expanded", () => {
    const roots = nest(tree);
    expect(visibleRows(roots, new Set()).map((r) => r.id)).toEqual(["a", "d"]);
    expect(visibleRows(roots, new Set(["a"])).map((r) => r.id)).toEqual(["a", "b", "d"]);
    expect(visibleRows(roots, new Set(["a", "b"])).map((r) => r.id)).toEqual([
      "a",
      "b",
      "c",
      "d",
    ]);
  });

  it("a collapsed ancestor hides its whole branch", () => {
    const roots = nest(tree);
    // "b" open but "a" shut: nothing under a is reachable.
    expect(visibleRows(roots, new Set(["b"])).map((r) => r.id)).toEqual(["a", "d"]);
  });
});

describe("breadcrumbOf", () => {
  it("is root-first and includes the page", () => {
    expect(breadcrumbOf(tree, "c").map((p) => p.id)).toEqual(["a", "b", "c"]);
  });

  it("is bounded, so a corrupt chain cannot hang the render", () => {
    const cyclic = [page("p", "q"), page("q", "p")];
    expect(breadcrumbOf(cyclic, "p").length).toBeLessThanOrEqual(16);
  });
});

describe("moving pages", () => {
  it("a page cannot move inside its own subtree", () => {
    expect(wouldCycle(tree, "a", "c")).toBe(true);
    expect(wouldCycle(tree, "a", "a")).toBe(true);
  });

  it("moving to the top level is always legal", () => {
    expect(wouldCycle(tree, "a", null)).toBe(false);
  });

  it("an unrelated destination is fine", () => {
    expect(wouldCycle(tree, "d", "a")).toBe(false);
  });

  it("descendants include the page itself", () => {
    expect([...descendantsOf(tree, "a")].sort()).toEqual(["a", "b", "c"]);
  });

  /** Moving a page moves everything under it, so a two-deep subtree dropped
   *  near the limit would push its leaves past it. */
  it("the depth cap accounts for the subtree being carried", () => {
    expect(heightOf(tree, "a")).toBe(2);
    const legal = legalParents(tree, "a").map((p) => p.id);
    // "d" is at depth 0, so a + its two levels would land at 3 — allowed.
    expect(legal).toContain("d");
    // Nothing inside a's own subtree is ever offered.
    expect(legal).not.toContain("b");
    expect(legal).not.toContain("c");
    expect(legal).not.toContain("a");
  });

  it("refuses a destination that would breach the depth cap", () => {
    // A chain already at the limit: r0/r1/r2/r3/r4 is depth 0..4.
    const deep: TreePage[] = [
      page("r0", null),
      page("r1", "r0"),
      page("r2", "r1"),
      page("r3", "r2"),
      page("r4", "r3"),
      page("loose", null),
    ];
    expect(depthOf(deep, "r4")).toBe(4);
    // A leaf may sit under r4 (depth 5 = the cap) but not below it.
    expect(legalParents(deep, "loose").map((p) => p.id)).toContain("r4");
    const withChild = [...deep, page("kid", "loose")];
    expect(legalParents(withChild, "loose").map((p) => p.id)).not.toContain("r4");
  });
});
