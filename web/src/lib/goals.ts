/** A goal as the server sends it. See `aichip_core::goals`. */
export interface Goal {
  id: string;
  parentId: string | null;
  title: string;
  description: string;
  status: "active" | "achieved" | "abandoned";
  targetDate: string | null;
  position: number;
  /** Cards in this goal's subtree that are done, and all of them. */
  done: number;
  total: number;
  createdAt: string;
}

export interface GoalRow {
  goal: Goal;
  depth: number;
  hasChildren: boolean;
}

/**
 * The tree, flattened in reading order: each goal followed by its children,
 * by position. A goal whose parent is missing reads as a top-level one, and
 * a loop in bad data is cut rather than followed forever.
 */
export function flatten(goals: Goal[]): GoalRow[] {
  const ids = new Set(goals.map((g) => g.id));
  const kids = new Map<string | null, Goal[]>();
  for (const g of goals) {
    const parent = g.parentId && ids.has(g.parentId) && g.parentId !== g.id ? g.parentId : null;
    kids.set(parent, [...(kids.get(parent) ?? []), g]);
  }
  for (const list of kids.values()) list.sort((a, b) => a.position - b.position || a.createdAt.localeCompare(b.createdAt));
  const out: GoalRow[] = [];
  const seen = new Set<string>();
  const walk = (parent: string | null, depth: number) => {
    for (const g of kids.get(parent) ?? []) {
      if (seen.has(g.id)) continue;
      seen.add(g.id);
      out.push({ goal: g, depth, hasChildren: (kids.get(g.id) ?? []).length > 0 });
      walk(g.id, depth + 1);
    }
  };
  walk(null, 0);
  for (const g of goals) if (!seen.has(g.id)) out.push({ goal: g, depth: 0, hasChildren: false });
  return out;
}

/** 0–100, or null when there is nothing to count yet. */
export function progress(g: Pick<Goal, "done" | "total">): number | null {
  return g.total === 0 ? null : Math.round((g.done / g.total) * 100);
}

/** Goals a goal may move under: not itself, and nothing beneath it. */
export function parentChoices(goals: Goal[], moving: string | null): Goal[] {
  if (!moving) return goals;
  const below = new Set<string>([moving]);
  let grew = true;
  while (grew) {
    grew = false;
    for (const g of goals) {
      if (g.parentId && below.has(g.parentId) && !below.has(g.id)) {
        below.add(g.id);
        grew = true;
      }
    }
  }
  return goals.filter((g) => !below.has(g.id));
}

/** "due in 12 days", "3 days overdue", "due today". */
export function dueLine(targetDate: string | null, today = new Date()): string | null {
  if (!targetDate) return null;
  const [y, m, d] = targetDate.split("-").map(Number);
  const due = Date.UTC(y, m - 1, d);
  const now = Date.UTC(today.getUTCFullYear(), today.getUTCMonth(), today.getUTCDate());
  const days = Math.round((due - now) / 86_400_000);
  if (days === 0) return "due today";
  if (days > 0) return `due in ${days} day${days === 1 ? "" : "s"}`;
  return `${-days} day${days === -1 ? "" : "s"} overdue`;
}
