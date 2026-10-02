import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import * as RD from "@radix-ui/react-dialog";
import { Outlet, useLocation } from "react-router-dom";
import { Sidebar } from "./components/shell/Sidebar";
import { TopBar } from "./components/shell/TopBar";
import { CommandPalette } from "./components/shell/CommandPalette";
import { NARROW, useMediaQuery } from "./lib/useMediaQuery";
import { readCollapsed, writeCollapsed } from "./lib/nav";
import { CrumbsProvider } from "./lib/crumbs";

/**
 * Sidebar, top bar, page. On a wide screen the sidebar is docked and can fold
 * to an icon rail; on a narrow one it is a drawer behind the top bar's menu
 * button. ⌘K opens the palette from anywhere.
 */
export default function AppShell() {
  const narrow = useMediaQuery(NARROW);
  const [navOpen, setNavOpen] = useState(false);
  const [collapsed, setCollapsed] = useState(readCollapsed);
  const [palette, setPalette] = useState(false);
  const { pathname } = useLocation();

  // Navigating is why the drawer was opened; leaving it over the destination
  // would mean two taps to get anywhere.
  useEffect(() => setNavOpen(false), [pathname]);
  useEffect(() => {
    if (!narrow) setNavOpen(false);
  }, [narrow]);

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        setPalette((p) => !p);
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, []);

  const toggle = useCallback(() => {
    setCollapsed((c) => {
      writeCollapsed(!c);
      return !c;
    });
  }, []);

  const page = (
    <div className="flex min-h-0 min-w-0 flex-1 flex-col">
      <TopBar onOpenPalette={() => setPalette(true)} onOpenNav={narrow ? () => setNavOpen(true) : undefined} />
      <main className="min-h-0 min-w-0 flex-1 overflow-hidden bg-bg">
        <RouteFade />
      </main>
    </div>
  );

  return (
    <CrumbsProvider>
      <div className="flex h-full min-h-0">
        {!narrow && (
          <div className={collapsed ? "w-14 shrink-0" : "w-[232px] shrink-0"}>
            <Sidebar collapsed={collapsed} onToggle={toggle} />
          </div>
        )}
        {page}
      </div>

      {narrow && (
        <RD.Root open={navOpen} onOpenChange={setNavOpen}>
          <RD.Portal>
            <RD.Overlay className="fixed inset-0 z-40 bg-[color-mix(in_oklab,black_35%,transparent)] data-[state=open]:animate-[fade-in_var(--dur-fast)_var(--ease-out-soft)]" />
            <RD.Content
              className="fixed inset-y-0 left-0 z-50 w-[264px] max-w-[85vw] shadow-[var(--shadow-lg)] outline-none"
              aria-describedby={undefined}
            >
              <RD.Title className="sr-only">Navigation</RD.Title>
              <Sidebar onNavigate={() => setNavOpen(false)} />
            </RD.Content>
          </RD.Portal>
        </RD.Root>
      )}

      <CommandPalette open={palette} onOpenChange={setPalette} />
    </CrumbsProvider>
  );
}

/**
 * Cross-fade between routes, keyed on the top path segment: moving between two
 * cards inside one project should feel like the page updating, not leaving.
 */
function RouteFade() {
  const { pathname } = useLocation();
  const section = pathname.split("/")[1] ?? "";
  return (
    <AnimatePresence mode="wait" initial={false}>
      <motion.div key={section} className="h-full min-h-0">
        <Outlet />
      </motion.div>
    </AnimatePresence>
  );
}
