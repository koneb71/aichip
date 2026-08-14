/**
 * Turning a project's files and imports into something a person can look at.
 *
 * All the arithmetic lives here and none of it in the canvas, for the reason
 * `workflowGraph.ts` does the same: this repository has no jsdom and no
 * testing-library, so anything inside a `.tsx` is permanently unverifiable.
 * If a claim about the picture is worth making, it is made here.
 *
 * Three decisions, each measured against this repository rather than guessed:
 *
 * 1. **Modules first, files on demand.** 287 source files with one node of
 *    in-degree 93 is a hairball no layout survives. Aggregated to modules it
 *    is 9 nodes and 13 edges — and it is a DAG, so its layers *are* the
 *    architecture. You expand into files when you want files.
 * 2. **Position is a pure function of the data.** No simulation, no random
 *    seed, no iteration over an object whose key order is not guaranteed. A
 *    map whose nodes move between visits destroys the one thing a map earns
 *    with repeated use, and makes "did the structure change?" unanswerable.
 * 3. **Size means one thing and the legend says which.** How many files import
 *    this one — not bytes, which would promote long files over load-bearing
 *    ones. Encoded through a band and a square root so that area, not radius,
 *    tracks the number.
 */

/** A file, as the graph endpoint sends it. */
export interface GraphFile {
  path: string;
  lang: string | null;
  bytes: number;
  rank: number;
  status: string;
  symbols: number;
  importedBy: number;
  imports: number;
}

/** One file importing another, `weight` specifiers deep. */
export interface GraphEdge {
  from: string;
  to: string;
  weight: number;
}

/** A node the canvas draws: a whole module, or one file inside an open one. */
export interface PlacedNode {
  id: string;
  kind: "module" | "file";
  /** What to write on it. */
  label: string;
  /** The module this belongs to — its own id when `kind` is "module". */
  module: string;
  x: number;
  y: number;
  /** Half the node's width; also the sunflower packing radius. */
  r: number;
  /** 0–7. What the size encodes, bucketed so small changes do not resize. */
  band: number;
  importedBy: number;
  imports: number;
  files: number;
  lang: string | null;
  expanded: boolean;
}

/** An edge between two *visible* nodes, folded if either end is collapsed. */
export interface PlacedEdge {
  id: string;
  from: string;
  to: string;
  /** How many real file-to-file imports this one line stands for. */
  weight: number;
  /** 1–6px, log-scaled. The width is a hint; `weight` is the truth. */
  width: number;
  /** Part of a cycle between modules. Drawn differently, never hidden. */
  cyclic: boolean;
}

export interface LaidOut {
  nodes: PlacedNode[];
  edges: PlacedEdge[];
  /** Module ids in layer order, for breadcrumbs and for keyboard traversal. */
  layers: string[][];
  /** Module pairs that import each other. Named, because a layered picture
   *  cannot show them and silently dropping one would flatter the design. */
  cycles: Array<[string, string]>;
}

/**
 * The module a path belongs to: its first two segments, or its first if that
 * is all there is.
 *
 * Two rather than one because one puts all 139 of this project's web files in
 * a single bucket and all 148 Rust files in another, which is a picture of the
 * languages and not of the code.
 */
export function moduleOf(path: string): string {
  const parts = path.split("/");
  // A file at the top level belongs to the repository, not to a module of its
  // own. Without this, `vite.config.ts`, `bunfig.toml` and eleven of their
  // neighbours each become a one-file module and the picture is mostly
  // punctuation.
  if (parts.length === 1) return ROOT_MODULE;
  return parts.length > 2 ? `${parts[0]}/${parts[1]}` : parts[0];
}

/** Where a file that lives at the repository root belongs. */
export const ROOT_MODULE = "/";

const GOLDEN_ANGLE = Math.PI * (3 - Math.sqrt(5)); // ≈137.507°

/**
 * Where the `i`th of `n` items sits in a disc, packed evenly.
 *
 * The sunflower arrangement: turn by the golden angle each step and push out
 * by √i, which fills a disc at near-uniform density with no two items landing
 * on the same spoke. `i = 0` is the centre, so ordering the input by
 * importance puts the most-depended-on file in the middle of its module.
 */
export function sunflower(i: number, n: number, radius: number): { x: number; y: number } {
  if (n <= 1) return { x: 0, y: 0 };
  const t = Math.sqrt(i / (n - 1));
  const a = i * GOLDEN_ANGLE;
  return { x: Math.cos(a) * t * radius, y: Math.sin(a) * t * radius };
}

/** How wide a disc has to be to hold `n` items at a comfortable spacing. */
function discRadius(n: number, itemR: number): number {
  return Math.max(itemR * 3, itemR * 2.4 * Math.sqrt(Math.max(n, 1)));
}

/**
 * Which of eight bands a value falls in, by rank among its peers.
 *
 * Bands rather than the raw number, because raw numbers make the picture
 * twitch: one file gaining an importer would resize half the canvas. A band
 * only changes when the ordering genuinely does.
 */
export function bandOf(value: number, sortedAscending: number[]): number {
  if (sortedAscending.length === 0) return 0;
  let below = 0;
  while (below < sortedAscending.length && sortedAscending[below] < value) below++;
  return Math.min(7, Math.floor((below / sortedAscending.length) * 8));
}

/**
 * Layer the module graph by longest path from a root.
 *
 * `edges` are `[before, after]` — the thing that must be placed first, then
 * the thing that depends on it. Callers pass a dependency as `[imported,
 * importer]`, which puts foundations on the left and the code built on them to
 * the right; laying it out the other way round is equally valid arithmetic and
 * reads backwards to everyone.
 *
 * Kahn's algorithm, so a cycle cannot spin it — and when one does appear, it
 * is **broken at its weakest link** rather than reported wholesale. Measured
 * on a real repository: `src/components` imports `src/lib` 85 times and
 * `src/lib` imports back 3 times. Refusing to layer, or flagging both
 * directions, paints one lopsided cycle as a graph-wide failure; cutting the
 * 3 leaves a readable picture and one honest red arrow.
 *
 * The cut edges come back as `cycles`, in *import* direction, so the caller
 * can draw exactly those differently. Nothing is hidden — a cut edge is still
 * rendered, just marked.
 */
export interface ModuleLink {
  /** Placed first — the module that is imported. */
  before: string;
  /** Placed after — the module that imports it. */
  after: string;
  /** How many file-level imports this stands for. Decides which link to cut. */
  weight: number;
}

export function layerModules(
  modules: string[],
  links: ModuleLink[],
): { layers: string[][]; cycles: Array<[string, string]> } {
  const sorted = [...modules].sort();
  const known = new Set(sorted);

  // Merged first: a hundred file imports between two modules is one link, and
  // counting it a hundred times would report a hundred cycles.
  const merged = new Map<string, ModuleLink>();
  for (const l of links) {
    if (l.before === l.after || !known.has(l.before) || !known.has(l.after)) continue;
    const k = `${l.before} ${l.after}`;
    const seen = merged.get(k);
    if (seen) seen.weight += l.weight;
    else merged.set(k, { ...l });
  }
  const live = [...merged.values()].sort(
    (a, b) => a.before.localeCompare(b.before) || a.after.localeCompare(b.after),
  );

  const cut = new Set<string>();
  let layer = new Map<string, number>();
  // Bounded by the number of links: every round either finishes or cuts one.
  for (let round = 0; round <= live.length; round++) {
    const active = live.filter((l) => !cut.has(`${l.before} ${l.after}`));
    const pass = kahn(sorted, active);
    if (pass.stuck.size === 0) {
      layer = pass.layer;
      break;
    }
    // The weakest link wholly inside the deadlock. Ties by name, so the same
    // repository always cuts the same edge.
    const inside = active
      .filter((l) => pass.stuck.has(l.before) && pass.stuck.has(l.after))
      .sort(
        (a, b) =>
          a.weight - b.weight ||
          `${a.before} ${a.after}`.localeCompare(`${b.before} ${b.after}`),
      );
    layer = pass.layer;
    if (!inside.length) break; // nothing left to cut; place what we have
    cut.add(`${inside[0].before} ${inside[0].after}`);
  }

  const depth = Math.max(0, ...sorted.map((m) => layer.get(m) ?? 0));
  const layers: string[][] = Array.from({ length: depth + 1 }, () => []);
  for (const m of sorted) layers[layer.get(m) ?? 0].push(m);
  // Returned the way an import reads — importer first — so a caller keyed on
  // its own edge direction matches without flipping anything.
  const cycles = [...cut].map((k) => {
    const [before, after] = k.split(" ");
    return [after, before] as [string, string];
  });
  return { layers, cycles };
}

/** One longest-path pass. `stuck` is whatever a cycle held back. */
function kahn(
  modules: string[],
  links: ModuleLink[],
): { layer: Map<string, number>; stuck: Set<string> } {
  const indegree = new Map(modules.map((m) => [m, 0]));
  const out = new Map(modules.map((m) => [m, [] as string[]]));
  for (const l of links) {
    out.get(l.before)!.push(l.after);
    indegree.set(l.after, indegree.get(l.after)! + 1);
  }
  const layer = new Map(modules.map((m) => [m, 0]));
  const settled = new Set<string>();
  // Sorted, so ties are broken by name and never by insertion order.
  let ready = modules.filter((m) => indegree.get(m) === 0).sort();
  while (ready.length) {
    const next: string[] = [];
    for (const m of ready) {
      settled.add(m);
      for (const t of out.get(m)!.sort()) {
        layer.set(t, Math.max(layer.get(t)!, layer.get(m)! + 1));
        indegree.set(t, indegree.get(t)! - 1);
        if (indegree.get(t) === 0) next.push(t);
      }
    }
    ready = next.sort();
  }
  return { layer, stuck: new Set(modules.filter((m) => !settled.has(m))) };
}

/** Log-scaled, because this repository's heaviest module edge carries 141
 *  imports and its lightest carries 1, and 141 pixels beside 1 is not a
 *  drawing. Always rendered beside the integer it stands for. */
export function edgeWidth(weight: number, maxWeight: number): number {
  const MIN = 1;
  const MAX = 6;
  if (maxWeight <= 1) return MIN;
  const t = Math.log(1 + Math.max(weight, 1)) / Math.log(1 + maxWeight);
  return MIN + (MAX - MIN) * t;
}

const FILE_R = 9;
const MODULE_R = 56;
const LAYER_GAP = 130;
const SIBLING_GAP = 56;

/**
 * The whole picture, from the data and the set of open modules.
 *
 * Pure and total: the same arguments give the same coordinates in the same
 * order, every session, on any machine.
 */
export function layout(
  files: GraphFile[],
  edges: GraphEdge[],
  expanded: Set<string>,
): LaidOut {
  const byPath = new Map(files.map((f) => [f.path, f]));
  const moduleFiles = new Map<string, GraphFile[]>();
  for (const f of [...files].sort((a, b) => a.path.localeCompare(b.path))) {
    const m = moduleOf(f.path);
    if (!moduleFiles.has(m)) moduleFiles.set(m, []);
    moduleFiles.get(m)!.push(f);
  }
  const modules = [...moduleFiles.keys()].sort();

  // Layering is always computed over the *module* graph, open or not: what
  // the layers mean is the architecture, and that must not rearrange itself
  // because somebody opened a folder.
  // Reversed on purpose: `layerModules` wants "before, after", and what comes
  // before is what gets imported. Foundations on the left, the code that leans
  // on them to the right.
  const moduleLinks: ModuleLink[] = [];
  for (const e of edges) {
    const importer = moduleOf(e.from);
    const imported = moduleOf(e.to);
    if (importer !== imported) {
      moduleLinks.push({ before: imported, after: importer, weight: e.weight });
    }
  }
  const { layers, cycles } = layerModules(modules, moduleLinks);
  const cyclic = new Set(cycles.map(([importer, imported]) => `${importer} ${imported}`));

  // Size bands, computed over every file once so a file's size means the same
  // thing whichever module happens to be open.
  const degrees = files.map((f) => f.importedBy).sort((a, b) => a - b);
  const moduleDegrees = modules
    .map((m) => moduleFiles.get(m)!.reduce((n, f) => n + f.importedBy, 0))
    .sort((a, b) => a - b);

  const radiusOf = (m: string) =>
    expanded.has(m) ? discRadius(moduleFiles.get(m)!.length, FILE_R) + FILE_R : MODULE_R;

  // Columns are as wide as their widest member, so opening a module pushes
  // its neighbours apart instead of drawing on top of them.
  const colX: number[] = [];
  let x = 0;
  for (const layerModules_ of layers) {
    const w = Math.max(MODULE_R, ...layerModules_.map(radiusOf));
    colX.push(x + w);
    x += 2 * w + LAYER_GAP;
  }

  const nodes: PlacedNode[] = [];
  layers.forEach((members, li) => {
    const heights = members.map((m) => 2 * radiusOf(m));
    const total = heights.reduce((a, b) => a + b, 0) + SIBLING_GAP * (members.length - 1);
    let y = -total / 2;
    members.forEach((m, mi) => {
      const r = radiusOf(m);
      const cy = y + heights[mi] / 2;
      y += heights[mi] + SIBLING_GAP;
      const kids = moduleFiles.get(m)!;
      const open = expanded.has(m);
      nodes.push({
        id: m,
        kind: "module",
        label: m,
        module: m,
        x: colX[li],
        y: cy,
        r,
        band: bandOf(
          kids.reduce((n, f) => n + f.importedBy, 0),
          moduleDegrees,
        ),
        importedBy: kids.reduce((n, f) => n + f.importedBy, 0),
        imports: kids.reduce((n, f) => n + f.imports, 0),
        files: kids.length,
        lang: kids[0]?.lang ?? null,
        expanded: open,
      });
      if (!open) return;
      // Most-depended-on first, so index 0 — the centre of the disc — is the
      // file the rest of the module leans on.
      const ordered = [...kids].sort(
        (a, b) => b.importedBy - a.importedBy || a.path.localeCompare(b.path),
      );
      ordered.forEach((f, i) => {
        const p = sunflower(i, ordered.length, r - FILE_R * 2);
        nodes.push({
          id: f.path,
          kind: "file",
          label: f.path.slice(m.length + 1) || f.path,
          module: m,
          x: colX[li] + p.x,
          y: cy + p.y,
          r: FILE_R,
          band: bandOf(f.importedBy, degrees),
          importedBy: f.importedBy,
          imports: f.imports,
          files: 0,
          lang: f.lang,
          expanded: false,
        });
      });
    });
  });

  // An edge is drawn at the finest level where both of its ends are visible,
  // and folded into the enclosing module otherwise. Direction survives the
  // fold: `web/components → web/lib` carries 141 imports and the reverse
  // carries none, and summing them would erase the layering it proves.
  const visible = (path: string) =>
    expanded.has(moduleOf(path)) && byPath.has(path) ? path : moduleOf(path);
  const folded = new Map<string, PlacedEdge>();
  for (const e of edges) {
    const a = visible(e.from);
    const b = visible(e.to);
    if (a === b) continue; // inside a closed module: nothing to draw
    const id = `${a} ${b}`;
    const existing = folded.get(id);
    if (existing) {
      existing.weight += e.weight;
    } else {
      folded.set(id, {
        id,
        from: a,
        to: b,
        weight: e.weight,
        width: 1,
        cyclic: cyclic.has(`${moduleOf(e.from)} ${moduleOf(e.to)}`),
      });
    }
  }
  const placed = [...folded.values()].sort((p, q) => p.id.localeCompare(q.id));
  const max = Math.max(1, ...placed.map((e) => e.weight));
  for (const e of placed) e.width = edgeWidth(e.weight, max);

  return { nodes, edges: placed, layers, cycles };
}

/** Everything one hop from `id`, in either direction, plus `id` itself. */
export function neighbourhood(id: string, edges: PlacedEdge[]): Set<string> {
  const near = new Set<string>([id]);
  for (const e of edges) {
    if (e.from === id) near.add(e.to);
    if (e.to === id) near.add(e.from);
  }
  return near;
}
