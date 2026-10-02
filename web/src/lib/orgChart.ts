/**
 * The org chart's shape, decided here and tested here — the canvas only
 * draws what this returns (there is no jsdom in this repository, so logic in
 * a `.tsx` cannot be asserted about).
 */

export interface OrgNode {
  id: string;
  name: string;
  title: string | null;
  icon: string;
  color: string;
  engine: string | null;
  modelTier: string;
  status: "active" | "paused" | "retired" | "pending_approval";
  reportsTo: string | null;
  liveCard: { taskId: string; projectId: string; title: string } | null;
  runsToday: number;
  spendTodayUsd: number;
  heartbeatSecs: number | null;
  lastHeartbeatAt: string | null;
}

/** Agents not on the chart yet sit at least this many to a row. */
export const LONER_COLUMNS = 4;

/** A box's footprint, in canvas units. */
export const BOX = { w: 232, h: 104, gapX: 28, gapY: 72 };

export interface Laid {
  positions: Map<string, { x: number; y: number }>;
  edges: { from: string; to: string }[];
}

/**
 * A tidy tree: each subtree gets a span as wide as its leaves need, a parent
 * sits centred over its children, and separate trees stand side by side.
 * Someone whose manager is not on the chart (retired, another workspace) is a
 * root rather than lost.
 */
export function layoutTree(nodes: OrgNode[]): Laid {
  const ids = new Set(nodes.map((n) => n.id));
  const kids = new Map<string, OrgNode[]>();
  const roots: OrgNode[] = [];
  for (const n of [...nodes].sort((a, b) => a.name.localeCompare(b.name))) {
    if (n.reportsTo && ids.has(n.reportsTo) && n.reportsTo !== n.id) {
      kids.set(n.reportsTo, [...(kids.get(n.reportsTo) ?? []), n]);
    } else {
      roots.push(n);
    }
  }
  const positions = new Map<string, { x: number; y: number }>();
  const edges: { from: string; to: string }[] = [];
  const unit = BOX.w + BOX.gapX;
  const seen = new Set<string>();

  // Leaves wide, measured once; a cycle in bad data is cut where it closes.
  const width = (id: string, path: Set<string>): number => {
    if (path.has(id)) return 1;
    const children = kids.get(id) ?? [];
    if (children.length === 0) return 1;
    const next = new Set(path).add(id);
    return children.reduce((sum, c) => sum + width(c.id, next), 0);
  };

  const place = (n: OrgNode, left: number, depth: number) => {
    if (seen.has(n.id)) return;
    seen.add(n.id);
    const span = width(n.id, new Set());
    positions.set(n.id, { x: left + ((span - 1) * unit) / 2, y: depth * (BOX.h + BOX.gapY) });
    let cursor = left;
    for (const c of kids.get(n.id) ?? []) {
      if (seen.has(c.id)) continue;
      edges.push({ from: n.id, to: c.id });
      place(c, cursor, depth + 1);
      cursor += width(c.id, new Set([n.id])) * unit;
    }
  };

  // Trees side by side; agents with no manager and no reports — not on the
  // chart yet — in a compact grid beneath, so one real tree among many
  // loners is not shrunk to fit a row of strangers.
  const trees = roots.filter((r) => (kids.get(r.id) ?? []).length > 0);
  const loners = roots.filter((r) => (kids.get(r.id) ?? []).length === 0);
  let left = 0;
  let deepest = -1;
  const depthOf = (id: string, d: number, path: Set<string>): number => {
    if (path.has(id)) return d;
    const next = new Set(path).add(id);
    return Math.max(d, ...(kids.get(id) ?? []).map((c) => depthOf(c.id, d + 1, next)));
  };
  for (const r of trees) {
    place(r, left, 0);
    left += width(r.id, new Set()) * unit;
    deepest = Math.max(deepest, depthOf(r.id, 0, new Set()));
  }
  const cols = Math.max(LONER_COLUMNS, Math.round(left / unit));
  const top = (deepest + 1) * (BOX.h + BOX.gapY) + (trees.length > 0 ? BOX.gapY : 0);
  loners.forEach((r, i) => {
    seen.add(r.id);
    positions.set(r.id, {
      x: (i % cols) * unit,
      y: top + Math.floor(i / cols) * (BOX.h + BOX.gapX),
    });
  });
  left = Math.max(left, Math.min(loners.length, cols) * unit);
  // Anything only reachable through a cycle in bad data: drawn, not dropped.
  for (const n of nodes) {
    if (!seen.has(n.id)) {
      place(n, left, 0);
      left += unit;
    }
  }
  return { positions, edges };
}

/** Would `moving` reporting to `to` make a loop? The server refuses it too;
 *  this is so the drop target can say no before anything is sent. */
export function wouldCycle(nodes: OrgNode[], moving: string, to: string): boolean {
  if (moving === to) return true;
  const up = new Map(nodes.map((n) => [n.id, n.reportsTo]));
  let at: string | null | undefined = to;
  for (let i = 0; at && i < 64; i++) {
    if (at === moving) return true;
    at = up.get(at);
  }
  return false;
}

/** "3 runs · $0.42 today", or nothing for an idle agent. */
export function todayLine(n: Pick<OrgNode, "runsToday" | "spendTodayUsd">): string | null {
  if (n.runsToday === 0) return null;
  const runs = `${n.runsToday} run${n.runsToday === 1 ? "" : "s"}`;
  return n.spendTodayUsd > 0 ? `${runs} · $${n.spendTodayUsd.toFixed(2)} today` : `${runs} today`;
}

/**
 * The chart the server just sent, or the one already shown when nothing in it
 * changed — so a poll that brings no news rebuilds nothing, and a box a
 * person is holding is not snapped back to its slot under the cursor.
 */
export function freshChart(shown: OrgNode[] | null, fetched: OrgNode[]): OrgNode[] {
  return shown && JSON.stringify(shown) === JSON.stringify(fetched) ? shown : fetched;
}
