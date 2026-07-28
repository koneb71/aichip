import { useEffect, useState } from "react";
import { Outlet, useLocation } from "react-router-dom";
import { Sidebar } from "./components/sidebar/Sidebar";
import { NARROW, useMediaQuery } from "./lib/useMediaQuery";

export default function AppShell() {
  const narrow = useMediaQuery(NARROW);
  const [navOpen, setNavOpen] = useState(false);
  const { pathname } = useLocation();

  // Navigating is the whole reason the drawer was opened; leaving it over the
  // destination would mean two taps to get anywhere.
  useEffect(() => setNavOpen(false), [pathname]);

  // Nothing to overlay once the sidebar is permanently visible again.
  useEffect(() => {
    if (!narrow) setNavOpen(false);
  }, [narrow]);

  useEffect(() => {
    if (!navOpen) return;
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setNavOpen(false);
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navOpen]);

  if (!narrow) {
    return (
      <div className="grid h-full grid-cols-[240px_minmax(0,1fr)]">
        <Sidebar />
        <main className="min-h-0 min-w-0 overflow-hidden">
          <Outlet />
        </main>
      </div>
    );
  }

  return (
    <div className="flex h-full min-h-0 flex-col">
      <header className="flex shrink-0 items-center gap-2 border-b border-line bg-panel px-3 py-2">
        <button
          onClick={() => setNavOpen(true)}
          aria-label="Open navigation"
          aria-expanded={navOpen}
          className="rounded-lg px-2 py-1 text-lg leading-none text-ink-dim hover:bg-panel-2 hover:text-ink"
        >
          ☰
        </button>
        <span className="text-base font-bold tracking-tight">
          <span className="text-accent">ai</span>chip
        </span>
      </header>

      <main className="min-h-0 min-w-0 flex-1 overflow-hidden">
        <Outlet />
      </main>

      {navOpen && (
        <>
          <div
            className="fixed inset-0 z-40 bg-black/30"
            onClick={() => setNavOpen(false)}
          />
          <div className="fixed inset-y-0 left-0 z-50 flex w-[260px] max-w-[85vw] flex-col">
            <Sidebar onNavigate={() => setNavOpen(false)} />
          </div>
        </>
      )}
    </div>
  );
}
