import { afterEach, describe, expect, it, vi } from "vitest";
import { NAV, readCollapsed, searchRows, sectionFor, writeCollapsed } from "./nav";

describe("sectionFor", () => {
  it("matches Home only exactly", () => {
    expect(sectionFor("/")?.label).toBe("Home");
    expect(sectionFor("/nowhere")).toBeUndefined();
  });

  it("matches a page and everything under it", () => {
    expect(sectionFor("/projects")?.label).toBe("Projects");
    expect(sectionFor("/projects/abc")?.label).toBe("Projects");
    expect(sectionFor("/knowledge/p1/edit")?.label).toBe("Knowledge");
  });

  it("does not treat a shared prefix as a parent", () => {
    // `/apps` must not claim `/appsettings`, which would light the wrong row.
    expect(sectionFor("/appsettings")).toBeUndefined();
  });

  it("every entry is reachable and named once", () => {
    const paths = NAV.map((n) => n.to);
    expect(new Set(paths).size).toBe(paths.length);
    for (const n of NAV) expect(sectionFor(n.to)?.to).toBe(n.to);
  });
});

describe("searchRows", () => {
  it("sends a card to its project with the card open, in display order", () => {
    const rows = searchRows({
      projects: [{ id: "p", label: "repo", sublabel: "" }],
      tasks: [{ id: "t", label: "fix", sublabel: "Review", projectId: "p" }],
      workflows: [],
      agents: [{ id: "a", label: "Ada", sublabel: "" }],
      teams: [],
    });
    expect(rows.map((r) => [r.group, r.to])).toEqual([
      ["Projects", "/projects/p"],
      ["Cards", "/projects/p?task=t"],
      ["Agents", "/agents?agent=a"],
    ]);
    expect(rows[0].sublabel).toBeUndefined();
  });
});

describe("sidebar collapse", () => {
  afterEach(() => vi.unstubAllGlobals());

  it("survives storage that throws", () => {
    vi.stubGlobal("window", {
      localStorage: {
        getItem: () => {
          throw new Error("blocked");
        },
        setItem: () => {
          throw new Error("blocked");
        },
      },
    });
    expect(readCollapsed()).toBe(false);
    expect(() => writeCollapsed(true)).not.toThrow();
  });

  it("remembers the rail", () => {
    const store = new Map<string, string>();
    vi.stubGlobal("window", {
      localStorage: { getItem: (k: string) => store.get(k) ?? null, setItem: (k: string, v: string) => store.set(k, v) },
    });
    writeCollapsed(true);
    expect(readCollapsed()).toBe(true);
    writeCollapsed(false);
    expect(readCollapsed()).toBe(false);
  });
});
