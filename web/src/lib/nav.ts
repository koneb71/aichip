import {
  Activity,
  BookOpen,
  Bot,
  CalendarClock,
  FolderKanban,
  House,
  LayoutGrid,
  MessageSquare,
  Plug,
  Settings,
  Sparkles,
  Telescope,
  Users,
  type LucideIcon,
} from "lucide-react";
import type { SearchResults } from "./api";

/**
 * Where things are. One list, read by the sidebar, the top bar's breadcrumb
 * and the command palette, so the three can never disagree about what a page
 * is called or which group it lives in.
 *
 * Four groups, each a question: what is happening (Work), who does it
 * (Organization), what they know (Knowledge), how it is wired (System).
 */

export type NavGroup = "Work" | "Organization" | "Knowledge" | "System";

export interface NavItem {
  to: string;
  label: string;
  icon: LucideIcon;
  group: NavGroup;
  /** Match only the exact path (Home), not everything under it. */
  end?: boolean;
  /** Extra words the palette should match, e.g. "board" for Projects. */
  keywords?: string[];
}

export const GROUPS: NavGroup[] = ["Work", "Organization", "Knowledge", "System"];

export const NAV: NavItem[] = [
  { to: "/", label: "Home", icon: House, group: "Work", end: true, keywords: ["dashboard", "overview"] },
  { to: "/projects", label: "Projects", icon: FolderKanban, group: "Work", keywords: ["board", "repo", "cards"] },
  { to: "/chat", label: "Chat", icon: MessageSquare, group: "Work", keywords: ["assistant"] },
  { to: "/activity", label: "Activity", icon: Activity, group: "Work", keywords: ["runs", "spend", "budget", "queue"] },
  { to: "/agents", label: "Agents", icon: Bot, group: "Organization", keywords: ["people", "specialists"] },
  { to: "/teams", label: "Teams", icon: Users, group: "Organization" },
  { to: "/routines", label: "Routines", icon: CalendarClock, group: "Organization", keywords: ["cron", "schedule", "manager"] },
  { to: "/knowledge", label: "Knowledge", icon: BookOpen, group: "Knowledge", keywords: ["wiki", "kb", "docs"] },
  { to: "/research", label: "Research", icon: Telescope, group: "Knowledge" },
  { to: "/apps", label: "Apps", icon: LayoutGrid, group: "Knowledge" },
  { to: "/skills", label: "Skills", icon: Sparkles, group: "Knowledge" },
  { to: "/connections", label: "Connections", icon: Plug, group: "System", keywords: ["mcp", "github", "servers"] },
  { to: "/settings", label: "Settings", icon: Settings, group: "System", keywords: ["models", "permissions", "theme"] },
];

/** The nav entry a path belongs to: the longest `to` that prefixes it. */
export function sectionFor(pathname: string, items: NavItem[] = NAV): NavItem | undefined {
  let best: NavItem | undefined;
  for (const item of items) {
    const hit = item.end
      ? pathname === item.to
      : pathname === item.to || pathname.startsWith(item.to.endsWith("/") ? item.to : `${item.to}/`);
    if (hit && (!best || item.to.length > best.to.length)) best = item;
  }
  return best;
}

/** A search hit, flattened, with where choosing it should go. */
export interface SearchRow {
  id: string;
  group: string;
  label: string;
  sublabel?: string;
  to: string;
}

/** The palette's search results in display order. Tasks and workflows open their project. */
export function searchRows(r: SearchResults): SearchRow[] {
  const row = (group: string, to: (h: SearchResults["projects"][number]) => string) => (h: SearchResults["projects"][number]) => ({
    id: `${group}:${h.id}`,
    group,
    label: h.label,
    sublabel: h.sublabel || undefined,
    to: to(h),
  });
  return [
    ...r.projects.map(row("Projects", (h) => `/projects/${h.id}`)),
    ...r.tasks.map(row("Cards", (h) => (h.projectId ? `/projects/${h.projectId}?task=${h.id}` : "/projects"))),
    ...r.workflows.map(row("Workflows", (h) => (h.projectId ? `/projects/${h.projectId}` : "/projects"))),
    ...r.agents.map(row("Agents", (h) => `/agents?agent=${h.id}`)),
    ...r.teams.map(row("Teams", () => "/teams")),
  ];
}

const COLLAPSE_KEY = "aichip.sidebar.collapsed";

/** Whether the sidebar was left as an icon rail. Guarded: storage can throw. */
export function readCollapsed(): boolean {
  try {
    return window.localStorage.getItem(COLLAPSE_KEY) === "1";
  } catch {
    return false;
  }
}

export function writeCollapsed(collapsed: boolean) {
  try {
    window.localStorage.setItem(COLLAPSE_KEY, collapsed ? "1" : "0");
  } catch {
    // Forgotten on reload; the toggle still works now.
  }
}
