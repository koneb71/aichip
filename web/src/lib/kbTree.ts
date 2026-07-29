/**
 * The page tree's arithmetic, kept out of the component.
 *
 * Extracted because this project's vitest setup has no DOM environment — a
 * tree rendered in a component is a tree whose ordering, nesting and cycle
 * rules cannot be tested at all. These are the parts worth being sure about.
 */

export interface TreePage {
  id: string;
  parentId: string | null;
  title: string;
  icon: string;
  position: number;
  status: string;
  origin: string;
  childCount: number;
  hasPending: boolean;
  writing: boolean;
}

export interface TreeNode extends TreePage {
  depth: number;
  children: TreeNode[];
}

/**
 * Turn the server's flat, ordered list into a tree.
 *
 * A page whose parent isn't in this list is lifted to the root rather than
 * dropped: a child in a space its parent doesn't belong to would otherwise
 * vanish from every view with nothing to say where it went.
 */
export function nest(pages: TreePage[]): TreeNode[] {
  const byId = new Map<string, TreeNode>();
  for (const p of pages) byId.set(p.id, { ...p, depth: 0, children: [] });

  const roots: TreeNode[] = [];
  for (const p of pages) {
    const node = byId.get(p.id)!;
    const parent = p.parentId ? byId.get(p.parentId) : undefined;
    if (parent) parent.children.push(node);
    else roots.push(node);
  }

  // Depth is assigned by walking, not by trusting parentId chains — a cycle
  // that survived the server's check would otherwise recurse forever here.
  const seen = new Set<string>();
  const walk = (nodes: TreeNode[], depth: number) => {
    for (const n of nodes) {
      if (seen.has(n.id)) {
        n.children = [];
        continue;
      }
      seen.add(n.id);
      n.depth = depth;
      walk(n.children, depth + 1);
    }
  };
  walk(roots, 0);
  return roots;
}

/** The rows a tree shows, given which nodes are open. */
export function visibleRows(nodes: TreeNode[], open: Set<string>): TreeNode[] {
  const out: TreeNode[] = [];
  const walk = (list: TreeNode[]) => {
    for (const n of list) {
      out.push(n);
      if (open.has(n.id)) walk(n.children);
    }
  };
  walk(nodes);
  return out;
}

/** Root-first path to a page, including it. */
export function breadcrumbOf(pages: TreePage[], id: string): TreePage[] {
  const byId = new Map(pages.map((p) => [p.id, p]));
  const path: TreePage[] = [];
  let cursor = byId.get(id);
  // Bounded: a corrupted parent chain must not hang the render.
  for (let i = 0; cursor && i < 16; i++) {
    path.unshift(cursor);
    cursor = cursor.parentId ? byId.get(cursor.parentId) : undefined;
  }
  return path;
}

/** Every id at or below a page, so a move picker can exclude them. */
export function descendantsOf(pages: TreePage[], id: string): Set<string> {
  const kids = new Map<string, string[]>();
  for (const p of pages) {
    if (!p.parentId) continue;
    kids.set(p.parentId, [...(kids.get(p.parentId) ?? []), p.id]);
  }
  const out = new Set<string>([id]);
  const stack = [id];
  while (stack.length) {
    for (const child of kids.get(stack.pop()!) ?? []) {
      if (out.has(child)) continue; // a cycle would spin here
      out.add(child);
      stack.push(child);
    }
  }
  return out;
}

/**
 * Would moving `moving` under `parent` create a loop?
 *
 * Checked on the server too. This copy exists so the move picker can refuse to
 * *offer* an illegal destination, which is a better experience than offering
 * one and rejecting the click.
 */
export function wouldCycle(pages: TreePage[], moving: string, parent: string | null): boolean {
  if (!parent) return false;
  return descendantsOf(pages, moving).has(parent);
}

/** How deep a page sits, 0 at the root. */
export function depthOf(pages: TreePage[], id: string): number {
  return breadcrumbOf(pages, id).length - 1;
}

/** The tallest branch below a page — moving it moves all of that too. */
export function heightOf(pages: TreePage[], id: string): number {
  const byParent = new Map<string, TreePage[]>();
  for (const p of pages) {
    if (!p.parentId) continue;
    byParent.set(p.parentId, [...(byParent.get(p.parentId) ?? []), p]);
  }
  const seen = new Set<string>();
  const walk = (node: string): number => {
    if (seen.has(node)) return 0;
    seen.add(node);
    const kids = byParent.get(node) ?? [];
    return kids.length ? 1 + Math.max(...kids.map((k) => walk(k.id))) : 0;
  };
  return walk(id);
}

/** Matches the server's cap. */
export const MAX_DEPTH = 5;

/** Destinations a page may legally move to. */
export function legalParents(pages: TreePage[], moving: string): TreePage[] {
  const height = heightOf(pages, moving);
  const banned = descendantsOf(pages, moving);
  return pages.filter(
    (p) => !banned.has(p.id) && depthOf(pages, p.id) + 1 + height <= MAX_DEPTH,
  );
}
