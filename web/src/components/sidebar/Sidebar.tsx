import { useEffect, useState } from "react";
import { NavLink, useLocation, useNavigate } from "react-router-dom";
import { motion } from "framer-motion";
import { api, App, Project } from "../../lib/api";
import { useActivity } from "../../lib/activity";
import { isWorking } from "../../lib/runStatus";
import { useWorkspace } from "../../lib/workspace";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SearchPalette } from "./SearchPalette";
import { UsageChip } from "../UsageChip";

const NAV = [
  { to: "/", label: "Home", icon: "⌂", end: true },
  { to: "/projects", label: "Projects", icon: "▤", end: false },
  { to: "/activity", label: "Activity", icon: "◈", end: false },
  { to: "/agents", label: "Agents", icon: "◉", end: false },
  { to: "/teams", label: "Teams", icon: "◫", end: false },
  { to: "/apps", label: "Apps", icon: "◳", end: false },
  { to: "/knowledge", label: "Knowledge", icon: "▦", end: false },
  { to: "/connections", label: "Connections", icon: "⚯", end: false },
  { to: "/settings", label: "Settings", icon: "⚙", end: false },
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
      <div className="mb-1 px-1 text-lg font-bold tracking-tight">
        <span className="text-accent">ai</span>chip
      </div>
      <WorkspaceSwitcher />

      <button
        onClick={() => {
          navigate("/projects?new=1");
          onNavigate?.();
        }}
        className="mt-2 flex items-center gap-2 rounded-lg border border-line bg-panel px-3 py-1.5 text-sm font-medium hover:bg-panel-2"
      >
        <span className="text-accent">+</span> New
      </button>

      <SearchPalette />

      <nav className="mt-3 flex flex-col gap-0.5">
        {NAV.map((item) => (
          <NavLink
            key={item.to}
            to={item.to}
            end={item.end}
            onClick={onNavigate}
            className={({ isActive }) =>
              `flex items-center gap-2.5 rounded-lg px-2.5 py-1.5 text-sm ${
                isActive
                  ? "bg-panel-2 font-semibold text-ink"
                  : "text-ink-dim hover:bg-panel-2 hover:text-ink"
              }`
            }
          >
            <span className="w-4 text-center">{item.icon}</span>
            {item.label}
            {item.to === "/activity" && <ActivityBadge />}
          </NavLink>
        ))}
      </nav>

      <div className="mt-1 flex min-h-0 flex-col overflow-y-auto">
        {/* An active app is one click away, and so is each screen it declares
            — a menu that only appears once you are already inside the app is
            a menu for somewhere you have arrived. */}
        {apps.length > 0 && (
          <>
            <div className="mt-4 px-2.5 text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
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
            <div className="mt-4 px-2.5 text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
              Recent
            </div>
            <div className="mt-1 flex flex-col gap-0.5">
              {recent.map((p) => (
                <NavLink
                  key={p.id}
                  to={`/projects/${p.id}`}
                  onClick={onNavigate}
                  className={({ isActive }) =>
                    `truncate rounded-lg px-2.5 py-1.5 text-sm ${
                      isActive
                        ? "bg-panel-2 font-medium text-ink"
                        : "text-ink-dim hover:bg-panel-2 hover:text-ink"
                    }`
                  }
                >
                  {p.name}
                </NavLink>
              ))}
            </div>
          </>
        )}
      </div>

      <div className="mt-auto" />
      <UsageChip />
      <div className="px-2 text-[11px] leading-relaxed text-ink-dim/70">
        Runs on your own Claude Code login.
        <br />
        No API keys, ever.
      </div>
    </aside>
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
    return <span className="ml-auto text-[11px] text-amber-600">capped</span>;
  }
  if (pulse.blocked > 0) {
    return (
      <motion.span
        initial={{ scale: 0.6 }}
        animate={{ scale: 1 }}
        className="ml-auto rounded-full bg-amber-500 px-1.5 text-[11px] font-semibold leading-4 text-white"
      >
        {pulse.blocked}
      </motion.span>
    );
  }
  if (pulse.paused) {
    return <span className="ml-auto text-[11px] text-amber-600">paused</span>;
  }
  if (pulse.working > 0) {
    return (
      <motion.span
        className="ml-auto h-1.5 w-1.5 rounded-full bg-tier-medium"
        animate={{ opacity: [1, 0.3, 1] }}
        transition={{ duration: 1.8, repeat: Infinity }}
      />
    );
  }
  return null;
}
