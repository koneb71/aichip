import { useEffect, useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import { motion } from "framer-motion";
import { api, App, Project } from "../../lib/api";
import { Icon, IconName } from "../ui/Icon";
import { springy, tappable } from "../../lib/motion";
import { useActivity } from "../../lib/activity";
import { isWorking } from "../../lib/runStatus";
import { useWorkspace } from "../../lib/workspace";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SearchPalette } from "./SearchPalette";
import { UsageChip } from "../UsageChip";
import { gradientFor } from "../ui/Surface";

/** Two groups with a rule between them: what you look at, then what you
 *  configure. Ten flat entries is a list you read every time; two groups of
 *  five is a shape you learn once. */
const NAV: { to: string; label: string; icon: IconName; end: boolean; group: 1 | 2 }[] = [
  { to: "/", label: "Home", icon: "home", end: true, group: 1 },
  { to: "/chat", label: "Chat", icon: "chat", end: false, group: 1 },
  { to: "/projects", label: "Projects", icon: "projects", end: false, group: 1 },
  { to: "/activity", label: "Activity", icon: "activity", end: false, group: 1 },
  { to: "/apps", label: "Apps", icon: "apps", end: false, group: 1 },
  { to: "/knowledge", label: "Knowledge", icon: "knowledge", end: false, group: 1 },
  { to: "/research", label: "Research", icon: "research", end: false, group: 1 },
  { to: "/agents", label: "Agents", icon: "agents", end: false, group: 2 },
  { to: "/skills", label: "Skills", icon: "skills", end: false, group: 2 },
  { to: "/teams", label: "Teams", icon: "teams", end: false, group: 2 },
  { to: "/connections", label: "Connections", icon: "connections", end: false, group: 2 },
  { to: "/settings", label: "Settings", icon: "settings", end: false, group: 2 },
];

/** `onNavigate` fires on anything that changes the route, so the narrow-screen
 *  drawer in AppShell can close itself. Undefined when docked. */
export function Sidebar({ onNavigate }: { onNavigate?: () => void } = {}) {
  const { active } = useWorkspace();
  const [recent, setRecent] = useState<Project[]>([]);
  const [apps, setApps] = useState<App[]>([]);
  const navigate = useNavigate();
  const location = useLocation();

  useEffect(() => {
    if (!active) return;
    api
      .projects(active.id)
      .then(({ projects }) => setRecent(projects.slice(0, 5)))
      .catch(() => setRecent([]));
  }, [active]);

  // Only the ones switched on. A deactivated app keeps its rows but is meant to
  // be out of the way, and a nav entry is the opposite of out of the way.
  //
  // Re-read on navigation rather than polled: installing, activating and
  // uninstalling all end with a route change, and a socket subscription for a
  // list this short would cost more than it saves.
  useEffect(() => {
    if (!active) return;
    api
      .apps(active.id)
      .then(({ apps }) => setApps(apps.filter((a) => a.active)))
      .catch(() => setApps([]));
  }, [active, location.pathname]);

  return (
    <aside className="flex h-full min-h-0 flex-col gap-1 border-r border-line bg-panel px-3 py-4">
      <div className="mb-2.5 flex items-center gap-2 px-1">
        <span className="grid size-7 place-items-center rounded-[9px] bg-accent text-[13px] font-black text-white">
          ai
        </span>
        <span className="text-[17px] font-bold tracking-tight">aichip</span>
      </div>
      <WorkspaceSwitcher />

      <motion.button
        {...tappable}
        onClick={() => {
          navigate("/projects?new=1");
          onNavigate?.();
        }}
        className="ring-focus mt-2.5 flex items-center gap-2.5 rounded-xl bg-accent px-3 py-2.5 text-sm font-semibold text-white shadow-[0_2px_10px_-2px_var(--color-accent)] transition-[filter] hover:brightness-110"
      >
        <span className="grid size-5 place-items-center rounded-md bg-white/20">
          <Icon name="plus" size={13} strokeWidth={2.5} />
        </span>
        New project
      </motion.button>

      <SearchPalette />

      <nav className="mt-3 flex flex-col gap-0.5">
        {NAV.filter((i) => i.group === 1).map((item) => (
          <NavItem key={item.to} item={item} onNavigate={onNavigate} />
        ))}
        <div className="mx-2.5 my-2 h-px bg-line" />
        {NAV.filter((i) => i.group === 2).map((item) => (
          <NavItem key={item.to} item={item} onNavigate={onNavigate} />
        ))}
      </nav>

      <div className="mt-1 flex min-h-0 flex-col overflow-y-auto">
        {/* An active app is one click away, and so is each screen it declares
            — a menu that only appears once you are already inside the app is
            a menu for somewhere you have arrived. */}
        {apps.length > 0 && (
          <>
            <div className="mt-5 px-2.5 text-[10px] font-semibold uppercase tracking-[0.09em] text-ink-dim/80">
              Your apps
            </div>
            <div className="mt-1 flex flex-col gap-0.5">
              {apps.map((app) => (
                <div key={app.id}>
                  <NavLink
                    to={`/apps/${app.id}`}
                    onClick={onNavigate}
                    className={({ isActive }) =>
                      `flex items-center gap-2 truncate rounded-lg px-2.5 py-1.5 text-sm ${
                        isActive
                          ? "bg-panel-2 font-medium text-ink"
                          : "text-ink-dim hover:bg-panel-2 hover:text-ink"
                      }`
                    }
                  >
                    <span className="w-4 shrink-0 text-center">{app.icon}</span>
                    <span className="truncate">{app.name}</span>
                  </NavLink>
                  {/* One entry is the page you just clicked, so listing it
                      underneath would say the same thing twice. */}
                  {app.menu.length > 1 && (
                    <div className="ml-6 flex flex-col gap-0.5 border-l border-line pl-2">
                      {app.menu.map((m) => (
                        <NavLink
                          key={m.view}
                          to={`/apps/${app.id}?view=${encodeURIComponent(m.view)}`}
                          onClick={onNavigate}
                          className="truncate rounded-lg px-2 py-1 text-xs text-ink-dim hover:bg-panel-2 hover:text-ink"
                        >
                          {m.label}
                        </NavLink>
                      ))}
                    </div>
                  )}
                </div>
              ))}
            </div>
          </>
        )}

        {recent.length > 0 && (
          <>
            <div className="mt-5 px-2.5 text-[10px] font-semibold uppercase tracking-[0.09em] text-ink-dim/80">
              Recent
            </div>
            <div className="mt-1 flex flex-col gap-0.5">
              {recent.map((p) => (
                <NavLink
                  key={p.id}
                  to={`/projects/${p.id}`}
                  onClick={onNavigate}
                  className={({ isActive }) =>
                    `flex items-center gap-2.5 truncate rounded-xl px-2.5 py-1.5 text-sm transition-colors ${
                      isActive
                        ? "bg-panel-2 font-medium text-ink"
                        : "text-ink-dim hover:bg-panel-2 hover:text-ink"
                    }`
                  }
                >
                  <span
                    className="size-2 shrink-0 rounded-[3px]"
                    style={{ background: gradientFor(p.name) }}
                  />
                  <span className="truncate">{p.name}</span>
                </NavLink>
              ))}
            </div>
          </>
        )}
      </div>

      <div className="mt-auto" />
      <UsageChip />
      <div className="mt-1 rounded-xl bg-panel-2/60 px-2.5 py-2 text-[11px] leading-relaxed text-ink-dim/80">
        Runs on your own Claude Code login.
        <br />
        No API keys, ever.
      </div>
    </aside>
  );
}

/**
 * One nav row.
 *
 * The active background is a `layoutId` element rather than a class, so moving
 * between pages slides the highlight from the old row to the new one instead of
 * blinking it out and in. Framer matches the two by id across the unmount, which
 * is the whole trick — there is only ever one of these mounted at a time.
 */
function NavItem({
  item,
  onNavigate,
}: {
  item: (typeof NAV)[number];
  onNavigate?: () => void;
}) {
  return (
    <NavLink
      to={item.to}
      end={item.end}
      onClick={onNavigate}
      className={({ isActive }) =>
        `ring-focus group relative flex items-center gap-2.5 rounded-xl px-2.5 py-2 text-sm transition-colors ${
          isActive ? "text-ink" : "text-ink-dim hover:text-ink"
        }`
      }
    >
      {({ isActive }) => (
        <>
          {isActive ? (
            <motion.span
              layoutId="nav-active"
              transition={springy}
              className="absolute inset-0 rounded-xl bg-panel-2"
            />
          ) : (
            <span className="absolute inset-0 rounded-xl opacity-0 transition-opacity group-hover:bg-panel-2 group-hover:opacity-60" />
          )}
          <span
            className={`relative transition-colors ${isActive ? "text-accent" : ""}`}
          >
            <Icon name={item.icon} size={17} />
          </span>
          <span className={`relative ${isActive ? "font-semibold" : ""}`}>{item.label}</span>
          {item.to === "/activity" && <ActivityBadge />}
        </>
      )}
    </NavLink>
  );
}

/** A count of things blocked on you, so you don't have to open the page to
 *  learn there's nothing to do. */
function ActivityBadge() {
  const { activity } = useActivity();
  if (!activity) return null;
  const pulse = {
    blocked: activity.blocked.length,
    working: activity.live.filter((r) => isWorking(r.status)).length,
    paused: activity.gate.state === "paused",
    overBudget: activity.gate.state === "over_budget",
  };

  if (pulse.overBudget) {
    return <span className="relative ml-auto text-[11px] text-amber-600">capped</span>;
  }
  if (pulse.blocked > 0) {
    return (
      <motion.span
        initial={{ scale: 0.6 }}
        animate={{ scale: 1 }}
        className="relative ml-auto rounded-full bg-amber-500 px-1.5 text-[11px] font-semibold leading-4 text-white"
      >
        {pulse.blocked}
      </motion.span>
    );
  }
  if (pulse.paused) {
    return <span className="relative ml-auto text-[11px] text-amber-600">paused</span>;
  }
  if (pulse.working > 0) {
    return (
      <motion.span
        className="relative ml-auto h-1.5 w-1.5 rounded-full bg-tier-medium"
        animate={{ opacity: [1, 0.3, 1] }}
        transition={{ duration: 1.8, repeat: Infinity }}
      />
    );
  }
  return null;
}
