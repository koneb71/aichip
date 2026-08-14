import { describe, expect, it } from "vitest";
import {
  bandOf,
  edgeWidth,
  GraphEdge,
  GraphFile,
  layerModules,
  layout,
  moduleOf,
  neighbourhood,
  ROOT_MODULE,
  sunflower,
} from "./repoGraph";

/**
 * Every claim here is about *relative* geometry — which node is left of which,
 * which is bigger, what moved and what did not. No test asserts a pixel: the
 * spacing constants are private and a suite that hardcodes them only tests
 * that nobody has changed its own copy.
 */

const file = (path: string, importedBy = 0, imports = 0): GraphFile => ({
  path,
  lang: path.endsWith(".rs") ? "rust" : "typescript",
  bytes: 100,
  rank: 0,
  status: "indexed",
  symbols: 1,
  importedBy,
  imports,
});

/** A small stand-in for the real shape: three modules in a chain. */
function project(): { files: GraphFile[]; edges: GraphEdge[] } {
  const files = [
    file("web/lib/api.ts", 2),
    file("web/lib/ws.ts", 1),
    file("web/components/Panel.tsx", 0, 2),
    file("web/components/Board.tsx", 0, 1),
    file("web/pages/Project.tsx", 0, 1),
  ];
  const edges: GraphEdge[] = [
    { from: "web/components/Panel.tsx", to: "web/lib/api.ts", weight: 3 },
    { from: "web/components/Panel.tsx", to: "web/lib/ws.ts", weight: 1 },
    { from: "web/components/Board.tsx", to: "web/lib/api.ts", weight: 1 },
    { from: "web/pages/Project.tsx", to: "web/components/Panel.tsx", weight: 1 },
  ];
  return { files, edges };
}

describe("moduleOf", () => {
  it("groups by the first two segments", () => {
    expect(moduleOf("crates/aichip-core/src/repo/index.rs")).toBe("crates/aichip-core");
    expect(moduleOf("web/src/lib/api.ts")).toBe("web/src");
  });

  it("puts a file at the repository root in one shared module", () => {
    // Otherwise vite.config.ts, bunfig.toml and their neighbours each become a
    // module of one and the picture is mostly punctuation.
    expect(moduleOf("vite.config.ts")).toBe(ROOT_MODULE);
    expect(moduleOf("README.md")).toBe(ROOT_MODULE);
    expect(moduleOf("backend/manage.py")).toBe("backend");
  });
});

/** `[before, after]` with a weight, the way `layout` builds them. */
const link = (before: string, after: string, weight = 1) => ({ before, after, weight });

describe("layerModules", () => {
  it("places each module after everything that must come before it", () => {
    // `layout` passes [imported, importer], so this is "lib before components
    // before pages".
    const { layers } = layerModules(
      ["web/lib", "web/components", "web/pages"],
      [link("web/lib", "web/components"), link("web/components", "web/pages")],
    );
    const depth = (m: string) => layers.findIndex((l) => l.includes(m));
    expect(depth("web/lib")).toBeLessThan(depth("web/components"));
    expect(depth("web/components")).toBeLessThan(depth("web/pages"));
  });

  it("breaks a cycle at its weakest link and says which one it cut", () => {
    // Measured on a real repository: components imports lib 85 times and lib
    // imports back 3. Refusing to layer, or flagging both directions, paints
    // one lopsided cycle as a graph-wide failure.
    const { layers, cycles } = layerModules(
      ["lib", "components"],
      [link("lib", "components", 85), link("components", "lib", 3)],
    );
    // Exactly one cut, and it is the light one — reported importer-first, so
    // "lib imports components" is the arrow that got marked.
    expect(cycles).toEqual([["lib", "components"]]);
    // The heavy direction still layers: lib is the foundation.
    const depth = (m: string) => layers.findIndex((l) => l.includes(m));
    expect(depth("lib")).toBeLessThan(depth("components"));
  });

  it("counts a hundred imports between two modules as one link, not a hundred", () => {
    const { cycles } = layerModules(
      ["a", "b"],
      [
        ...Array.from({ length: 50 }, () => link("a", "b")),
        ...Array.from({ length: 50 }, () => link("b", "a")),
      ],
    );
    expect(cycles).toHaveLength(1);
  });

  it("terminates on a three-module cycle", () => {
    const { layers, cycles } = layerModules(
      ["a", "b", "c"],
      [link("a", "b"), link("b", "c"), link("c", "a")],
    );
    expect(cycles).toHaveLength(1);
    expect(layers.flat().sort()).toEqual(["a", "b", "c"]);
  });

  it("does not depend on the order the links arrive in", () => {
    const forwards = layerModules(["a", "b", "c"], [link("b", "a"), link("c", "b")]);
    const backwards = layerModules(["c", "b", "a"], [link("c", "b"), link("b", "a")]);
    expect(forwards.layers).toEqual(backwards.layers);
  });

  it("ignores a link naming a module that is not there", () => {
    const { layers } = layerModules(["a"], [link("a", "ghost"), link("ghost", "a")]);
    expect(layers.flat()).toEqual(["a"]);
  });
});

describe("sunflower", () => {
  it("puts the first item at the centre and later ones further out", () => {
    const c = sunflower(0, 40, 100);
    expect(Math.hypot(c.x, c.y)).toBe(0);
    const near = sunflower(5, 40, 100);
    const far = sunflower(30, 40, 100);
    expect(Math.hypot(near.x, near.y)).toBeLessThan(Math.hypot(far.x, far.y));
  });

  it("never places two of ninety items on top of each other", () => {
    // aichip-core is 90 files; a naive ring at this count overlaps.
    const pts = Array.from({ length: 90 }, (_, i) => sunflower(i, 90, 200));
    for (let i = 0; i < pts.length; i++) {
      for (let j = i + 1; j < pts.length; j++) {
        expect(Math.hypot(pts[i].x - pts[j].x, pts[i].y - pts[j].y)).toBeGreaterThan(1);
      }
    }
  });

  it("stays inside the disc it was given", () => {
    for (let i = 0; i < 50; i++) {
      const p = sunflower(i, 50, 100);
      expect(Math.hypot(p.x, p.y)).toBeLessThanOrEqual(100.0001);
    }
  });
});

describe("bandOf", () => {
  it("does not resize the hub when a new leaf file appears", () => {
    // The reason bands exist: with raw numbers, adding one file to a project
    // rescales every node in it.
    const before = [0, 1, 2, 5, 9, 20, 40, 93];
    const after = [...before, 0, 0, 1].sort((a, b) => a - b);
    expect(bandOf(93, after)).toBe(bandOf(93, before));
    expect(bandOf(40, after)).toBe(bandOf(40, before));
  });

  it("puts the biggest in the top band and the smallest in the bottom", () => {
    const peers = [0, 1, 2, 5, 9, 20, 40, 93];
    expect(bandOf(93, peers)).toBe(7);
    expect(bandOf(0, peers)).toBe(0);
  });

  it("survives an empty project", () => {
    expect(bandOf(3, [])).toBe(0);
  });
});

describe("edgeWidth", () => {
  it("keeps a 141-to-1 range drawable", () => {
    // Measured on this repository: web/components → web/lib carries 141
    // imports, several pairs carry 1. Linear would demand a 141px stroke.
    const heavy = edgeWidth(141, 141);
    const light = edgeWidth(1, 141);
    expect(heavy).toBeGreaterThan(light);
    expect(heavy).toBeLessThanOrEqual(6);
    expect(light).toBeGreaterThanOrEqual(1);
    // Log-scaled: the midpoint by count is well past the midpoint by width.
    expect(edgeWidth(70, 141)).toBeGreaterThan((heavy + light) / 2);
  });

  it("does not divide by zero on a graph with one edge", () => {
    expect(Number.isFinite(edgeWidth(1, 1))).toBe(true);
  });
});

describe("layout", () => {
  it("draws modules, not files, until a module is opened", () => {
    const { files, edges } = project();
    const closed = layout(files, edges, new Set());
    expect(closed.nodes.every((n) => n.kind === "module")).toBe(true);
    expect(closed.nodes.map((n) => n.id).sort()).toEqual([
      "web/components",
      "web/lib",
      "web/pages",
    ]);
  });

  it("places a module to the right of everything it imports", () => {
    const { files, edges } = project();
    const { nodes } = layout(files, edges, new Set());
    const x = (id: string) => nodes.find((n) => n.id === id)!.x;
    expect(x("web/lib")).toBeLessThan(x("web/components"));
    expect(x("web/components")).toBeLessThan(x("web/pages"));
  });

  it("gives the same coordinates twice, and after the input is reordered", () => {
    // Spatial memory is the whole return on looking at a map more than once.
    const { files, edges } = project();
    const a = layout(files, edges, new Set());
    const b = layout([...files].reverse(), [...edges].reverse(), new Set());
    expect(a.nodes).toEqual(b.nodes);
    expect(a.edges).toEqual(b.edges);
  });

  it("opens a module into its files and puts the most-depended-on in the middle", () => {
    const { files, edges } = project();
    const open = layout(files, edges, new Set(["web/lib"]));
    const mod = open.nodes.find((n) => n.id === "web/lib")!;
    const api = open.nodes.find((n) => n.id === "web/lib/api.ts")!;
    const ws = open.nodes.find((n) => n.id === "web/lib/ws.ts")!;
    expect(api.kind).toBe("file");
    // api.ts has more importers, so it sits nearer its module's centre.
    expect(Math.hypot(api.x - mod.x, api.y - mod.y)).toBeLessThan(
      Math.hypot(ws.x - mod.x, ws.y - mod.y),
    );
    // The modules that are still shut stay whole.
    expect(open.nodes.find((n) => n.id === "web/components")!.kind).toBe("module");
    expect(open.nodes.some((n) => n.id === "web/components/Panel.tsx")).toBe(false);
  });

  it("opening a big module pushes its neighbours apart rather than drawing over them", () => {
    // Thirty files, because that is when the disc outgrows a shut module's
    // box — this repository's aichip-core is ninety.
    const files = [
      ...Array.from({ length: 30 }, (_, i) => file(`web/lib/f${i}.ts`, 1)),
      file("web/pages/Project.tsx", 0, 1),
    ];
    const edges: GraphEdge[] = [
      { from: "web/pages/Project.tsx", to: "web/lib/f0.ts", weight: 1 },
    ];
    const gap = (l: LaidOutLike) =>
      Math.abs(
        l.nodes.find((n) => n.id === "web/pages")!.x -
          l.nodes.find((n) => n.id === "web/lib")!.x,
      );
    expect(gap(layout(files, edges, new Set(["web/lib"])))).toBeGreaterThan(
      gap(layout(files, edges, new Set())),
    );
  });

  it("draws an edge at the finest level where both of its ends are visible", () => {
    const { files, edges } = project();
    const closed = layout(files, edges, new Set());
    // Both modules shut: one folded edge carrying every import between them.
    const folded = closed.edges.find(
      (e) => e.from === "web/components" && e.to === "web/lib",
    )!;
    expect(folded.weight).toBe(5); // 3 + 1 + 1

    // Open the target: the same imports now land on individual files.
    const open = layout(files, edges, new Set(["web/lib"]));
    expect(open.edges.some((e) => e.to === "web/lib/api.ts")).toBe(true);
    expect(open.edges.some((e) => e.from === "web/components")).toBe(true);
    // …and the folded version is gone, not drawn on top of them.
    expect(open.edges.some((e) => e.to === "web/lib")).toBe(false);
  });

  it("hides an edge that is entirely inside a closed module", () => {
    const files = [file("a/b/one.ts", 1), file("a/b/two.ts")];
    const edges: GraphEdge[] = [{ from: "a/b/two.ts", to: "a/b/one.ts", weight: 1 }];
    expect(layout(files, edges, new Set()).edges).toEqual([]);
    expect(layout(files, edges, new Set(["a/b"])).edges).toHaveLength(1);
  });

  it("keeps direction when it folds", () => {
    // web/components → web/lib is 141 imports and the reverse is none. A fold
    // that summed both would erase the layering it proves.
    const { files, edges } = project();
    const { edges: drawn } = layout(files, edges, new Set());
    expect(drawn.some((e) => e.from === "web/lib" && e.to === "web/components")).toBe(false);
  });

  it("sizes a node by how many files import it, not by how big it is", () => {
    const files = [
      { ...file("m/x/hub.ts", 40), bytes: 10 },
      { ...file("m/x/huge.ts", 0), bytes: 900_000 },
    ];
    const { nodes } = layout(files, [], new Set(["m/x"]));
    const hub = nodes.find((n) => n.id === "m/x/hub.ts")!;
    const huge = nodes.find((n) => n.id === "m/x/huge.ts")!;
    expect(hub.band).toBeGreaterThan(huge.band);
  });

  it("survives a project with nothing in it", () => {
    const empty = layout([], [], new Set());
    expect(empty.nodes).toEqual([]);
    expect(empty.edges).toEqual([]);
  });

  it("survives an edge naming a file that is not in the node list", () => {
    // The graph endpoint and the file list are two queries; a file deleted
    // between them must not crash the canvas.
    const files = [file("a/b/one.ts")];
    const edges: GraphEdge[] = [{ from: "a/b/one.ts", to: "gone/away/two.ts", weight: 1 }];
    expect(() => layout(files, edges, new Set(["a/b"]))).not.toThrow();
  });
});

describe("neighbourhood", () => {
  it("reaches one hop in both directions", () => {
    const { files, edges } = project();
    const { edges: drawn } = layout(files, edges, new Set());
    const near = neighbourhood("web/components", drawn);
    expect(near.has("web/lib")).toBe(true); // it imports
    expect(near.has("web/pages")).toBe(true); // it is imported by
    expect(near.has("web/components")).toBe(true);
  });
});

/** Structural alias so the helper above reads without importing the type. */
type LaidOutLike = ReturnType<typeof layout>;
