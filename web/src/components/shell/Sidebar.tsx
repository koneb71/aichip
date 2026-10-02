import { useEffect, useState } from "react";
import { NavLink, useLocation } from "react-router-dom";
import { ChevronsUpDown, Check, PanelLeftClose, PanelLeftOpen, Plus } from "lucide-react";
import { api, App, Project } from "../../lib/api";
import { GROUPS, NAV, type NavItem } from "../../lib/nav";
import { useActivity } from "../../lib/activity";
import { isWorking } from "../../lib/runStatus";
import { useWorkspace } from "../../lib/workspace";
import { useInbox } from "../../lib/inbox";
import { UsageChip } from "../UsageChip";
import { gradientFor } from "../ui/Surface";
import { cn } from "../ui/cn";
import { Tooltip, Menu } from "../ui/Overlay";
import { Input } from "../ui/Field";
import { Button } from "../ui/Button";

/**
 * The sidebar: four groups, recent projects and active apps, and the
 * workspace. It folds to a 56px icon rail — remembered — for people who live
 * on the board and want the width back.
 */
export function Sidebar({
  collapsed = false,
  onToggle,
  onNavigate,
}: {
  collapsed?: boolean;
  onToggle?: () => void;
  /** Fires on any route change, so the narrow-screen drawer can close. */
  onNavigate?: () => void;
}) {
  const { active } = useWorkspace();
  const [recent, setRecent] = useState<Project[]>([]);
  const [apps, setApps] = useState<App[]>([]);
  const location = useLocation();

  useEffect(() => {
    if (!active) return;
    api
      .projects(active.id)
      .then(({ projects }) => setRecent(projects.slice(0, 5)))
      .catch(() => setRecent([]));
  }, [active]);

  // Re-read on navigation rather than polled: installing, activating and
  // uninstalling an app all end with a route change.
  useEffect(() => {
    if (!active) return;
    api
      .apps(active.id)
      .then(({ apps }) => setApps(apps.filter((a) => a.active)))
      .catch(() => setApps([]));
  }, [active, location.pathname]);

  return (
    <aside
      className={cn(
        "flex h-full min-h-0 flex-col border-r border-border bg-panel",
        collapsed ? "w-14 items-center px-2" : "w-full px-2.5",
      )}
    >
      <div className={cn("flex h-12 shrink-0 items-center", collapsed ? "justify-center" : "justify-between gap-2 px-1")}>
        {!collapsed && <WorkspaceMenu />}
        {onToggle && (
          <Tooltip content={collapsed ? "Expand sidebar" : "Collapse sidebar"} side="right">
            <button
              type="button"
              onClick={onToggle}
              aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
              className="ring-focus grid size-7 shrink-0 place-items-center rounded-md text-fg-subtle hover:bg-panel-2 hover:text-fg"
            >
              {collapsed ? <PanelLeftOpen className="size-4" /> : <PanelLeftClose className="size-4" />}
            </button>
          </Tooltip>
        )}
      </div>

      <nav className={cn("flex min-h-0 flex-1 flex-col overflow-y-auto pb-2", collapsed && "items-center")} aria-label="Main">
        {GROUPS.map((group) => {
          const items = NAV.filter((n) => n.group === group);
          return (
            <div key={group} className={cn("flex flex-col gap-px", collapsed ? "mt-2 items-center" : "mt-3")}>
              {collapsed ? (
                <div className="mx-auto mb-1 h-px w-5 bg-border" />
              ) : (
                <div className="px-2 pb-1 text-[11px] font-medium text-fg-subtle">{group}</div>
              )}
              {items.map((item) => (
                <NavRow key={item.to} item={item} collapsed={collapsed} onNavigate={onNavigate} />
              ))}
            </div>
          );
        })}

        {!collapsed && apps.length > 0 && (
          <div className="mt-4 flex flex-col gap-px">
            <div className="px-2 pb-1 text-[11px] font-medium text-fg-subtle">Your apps</div>
            {apps.map((app) => (
              <div key={app.id}>
                <NavLink
                  to={`/apps/${app.id}`}
                  onClick={onNavigate}
                  className={({ isActive }) =>
                    cn(
                      "ring-focus flex h-7 items-center gap-2 truncate rounded-md px-2 text-[13px]",
                      isActive ? "bg-panel-2 font-medium text-fg" : "text-fg-muted hover:bg-panel-2 hover:text-fg",
                    )
                  }
                >
                  <span className="w-4 shrink-0 text-center text-xs">{app.icon}</span>
                  <span className="truncate">{app.name}</span>
                </NavLink>
                {app.menu.length > 1 && (
                  <div className="ml-[17px] flex flex-col gap-px border-l border-border pl-2">
                    {app.menu.map((m) => (
                      <NavLink
                        key={m.view}
                        to={`/apps/${app.id}?view=${encodeURIComponent(m.view)}`}
                        onClick={onNavigate}
                        className="ring-focus truncate rounded-md px-2 py-1 text-xs text-fg-muted hover:bg-panel-2 hover:text-fg"
                      >
                        {m.label}
                      </NavLink>
                    ))}
                  </div>
                )}
              </div>
            ))}
          </div>
        )}

        {!collapsed && recent.length > 0 && (
          <div className="mt-4 flex flex-col gap-px">
            <div className="px-2 pb-1 text-[11px] font-medium text-fg-subtle">Recent</div>
            {recent.map((p) => (
              <NavLink
                key={p.id}
                to={`/projects/${p.id}`}
                onClick={onNavigate}
                className={({ isActive }) =>
                  cn(
                    "ring-focus flex h-7 items-center gap-2 truncate rounded-md px-2 text-[13px]",
                    isActive ? "bg-panel-2 font-medium text-fg" : "text-fg-muted hover:bg-panel-2 hover:text-fg",
                  )
                }
              >
                <span className="size-2.5 shrink-0 rounded-[3px]" style={{ background: gradientFor(p.name) }} />
                <span className="truncate">{p.name}</span>
              </NavLink>
            ))}
          </div>
        )}
      </nav>

      {!collapsed && (
        <div className="shrink-0 pb-3">
          <UsageChip />
          <p className="px-2 text-[11px] leading-relaxed text-fg-subtle">Runs on your own CLI logins. No API keys, ever.</p>
        </div>
      )}
    </aside>
  );
}

function NavRow({ item, collapsed, onNavigate }: { item: NavItem; collapsed: boolean; onNavigate?: () => void }) {
  const Icon = item.icon;
  const link = (
    <NavLink
      to={item.to}
      end={item.end}
      onClick={onNavigate}
      aria-label={collapsed ? item.label : undefined}
      className={({ isActive }) =>
        cn(
          "ring-focus relative flex shrink-0 items-center rounded-md text-[13px] transition-colors duration-[var(--dur-fast)]",
          collapsed ? "size-8 justify-center" : "h-7 gap-2.5 px-2",
          isActive ? "bg-panel-2 font-medium text-fg" : "text-fg-muted hover:bg-panel-2 hover:text-fg",
        )
      }
    >
      {({ isActive }) => (
        <>
          <Icon className={cn("size-4 shrink-0", isActive ? "text-accent-fg" : "")} strokeWidth={isActive ? 2.1 : 1.8} />
          {!collapsed && <span className="truncate">{item.label}</span>}
          {item.to === "/activity" && <ActivityBadge collapsed={collapsed} />}
          {item.to === "/inbox" && <InboxBadge collapsed={collapsed} />}
        </>
      )}
    </NavLink>
  );
  return collapsed ? (
    <Tooltip content={item.label} side="right">
      {link}
    </Tooltip>
  ) : (
    link
  );
}

/** How many things are waiting on you that you have not looked at. */
function InboxBadge({ collapsed }: { collapsed: boolean }) {
  const { unread } = useInbox();
  if (!unread) return null;
  return (
    <span
      className={cn(
        collapsed ? "absolute right-0.5 top-0.5" : "ml-auto",
        "tabular grid h-4 min-w-4 place-items-center rounded-full bg-accent px-1 text-[10px] font-semibold leading-none text-on-accent",
      )}
    >
      {unread > 99 ? "99+" : unread}
    </span>
  );
}

/** What is going on, without opening the page: capped, a count waiting on you, paused, or a live pulse. */
function ActivityBadge({ collapsed }: { collapsed: boolean }) {
  const { activity } = useActivity();
  if (!activity) return null;
  const working = activity.live.filter((r) => isWorking(r.status)).length;
  const pos = collapsed ? "absolute right-0.5 top-0.5" : "ml-auto";
  if (activity.gate.state === "over_budget") {
    return collapsed ? (
      <span className={cn(pos, "size-2 rounded-full bg-danger")} />
    ) : (
      <span className="ml-auto text-[11px] font-medium text-danger-fg">capped</span>
    );
  }
  if (activity.gate.state === "paused") {
    return collapsed ? (
      <span className={cn(pos, "size-2 rounded-full bg-warning")} />
    ) : (
      <span className="ml-auto text-[11px] font-medium text-warning-fg">paused</span>
    );
  }
  if (working > 0) {
    return <span className={cn(pos, "size-1.5 animate-pulse rounded-full bg-accent")} />;
  }
  return null;
}

/** The workspace, as a menu: switch, or make a new one inline. */
function WorkspaceMenu() {
  const { workspaces, active, setActive, refresh } = useWorkspace();
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");

  const create = async () => {
    if (!name.trim()) return;
    const { id } = await api.createWorkspace(name.trim());
    await refresh();
    setActive(id);
    setName("");
    setCreating(false);
  };

  const initials = (active?.name ?? "W")
    .split(" ")
    .map((w) => w[0])
    .slice(0, 2)
    .join("")
    .toUpperCase();

  if (creating) {
    return (
      <form
        className="flex min-w-0 flex-1 items-center gap-1"
        onSubmit={(e) => {
          e.preventDefault();
          void create();
        }}
      >
        <Input autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Workspace name" className="h-7" onKeyDown={(e) => e.key === "Escape" && setCreating(false)} />
        <Button type="submit" size="sm" variant="primary">
          Add
        </Button>
      </form>
    );
  }

  return (
    <Menu
      align="start"
      label="Workspaces"
      trigger={
        <button
          type="button"
          className="ring-focus flex min-w-0 flex-1 items-center gap-2 rounded-md px-1.5 py-1 text-left hover:bg-panel-2"
        >
          <span
            className="grid size-6 shrink-0 place-items-center rounded-md text-[10px] font-semibold text-white"
            style={{ background: active?.color ?? "var(--color-accent)" }}
          >
            {initials}
          </span>
          <span className="min-w-0 flex-1 truncate text-[13px] font-semibold">{active?.name ?? "Workspace"}</span>
          <ChevronsUpDown className="size-3.5 shrink-0 text-fg-subtle" />
        </button>
      }
      items={[
        ...workspaces.map((w) => ({
          label: w.name,
          icon: <span className="size-2.5 rounded-full" style={{ background: w.color }} />,
          shortcut: w.id === active?.id ? <Check className="size-3.5" /> : undefined,
          onSelect: () => setActive(w.id),
        })),
        null,
        { label: "New workspace", icon: <Plus className="size-3.5" />, onSelect: () => setCreating(true) },
      ]}
    />
  );
}
