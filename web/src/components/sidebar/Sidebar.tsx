import { useEffect, useState } from "react";
import { NavLink, useNavigate } from "react-router-dom";
import { api, Project } from "../../lib/api";
import { useWorkspace } from "../../lib/workspace";
import { WorkspaceSwitcher } from "./WorkspaceSwitcher";
import { SearchPalette } from "./SearchPalette";

const NAV = [
  { to: "/", label: "Home", icon: "⌂", end: true },
  { to: "/projects", label: "Projects", icon: "▤", end: false },
  { to: "/agents", label: "Agents", icon: "◉", end: false },
  { to: "/teams", label: "Teams", icon: "◫", end: false },
];

export function Sidebar() {
  const { active } = useWorkspace();
  const [recent, setRecent] = useState<Project[]>([]);
  const navigate = useNavigate();

  useEffect(() => {
    if (!active) return;
    api
      .projects(active.id)
      .then(({ projects }) => setRecent(projects.slice(0, 5)))
      .catch(() => setRecent([]));
  }, [active]);

  return (
    <aside className="flex min-h-0 flex-col gap-1 border-r border-line bg-panel px-3 py-4">
      <div className="mb-1 px-1 text-lg font-bold tracking-tight">
        <span className="text-accent">ai</span>chip
      </div>
      <WorkspaceSwitcher />

      <button
        onClick={() => navigate("/projects?new=1")}
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
          </NavLink>
        ))}
      </nav>

      {recent.length > 0 && (
        <>
          <div className="mt-5 px-2.5 text-[11px] font-semibold uppercase tracking-wider text-ink-dim">
            Recent
          </div>
          <div className="mt-1 flex min-h-0 flex-col gap-0.5 overflow-y-auto">
            {recent.map((p) => (
              <NavLink
                key={p.id}
                to={`/projects/${p.id}`}
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

      <div className="mt-auto px-2 text-[11px] leading-relaxed text-ink-dim/70">
        Runs on your own Claude Code login.
        <br />
        No API keys, ever.
      </div>
    </aside>
  );
}
