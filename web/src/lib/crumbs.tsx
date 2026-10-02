import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import type { Crumb } from "../components/ui/Layout";

/**
 * What the top bar's breadcrumb says past the section name.
 *
 * The section ("Projects") comes from the route through `lib/nav`; only a page
 * knows the rest ("repo", "Fix login"), so a page that has a name to show
 * calls `useCrumbs` and the top bar picks it up. Leaving the page clears it.
 */

interface CrumbState {
  items: Crumb[];
  set: (c: Crumb[]) => void;
}

const Ctx = createContext<CrumbState>({ items: [], set: () => {} });

export function CrumbsProvider({ children }: { children: ReactNode }) {
  const [items, set] = useState<Crumb[]>([]);
  const value = useMemo(() => ({ items, set }), [items]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useCrumbItems(): Crumb[] {
  return useContext(Ctx).items;
}

/** Set the trail after the section for as long as this page is mounted. `key` re-sets when it changes. */
export function useCrumbs(items: Crumb[], key: string) {
  const { set } = useContext(Ctx);
  useEffect(() => {
    set(items);
    return () => set([]);
    // `items` is rebuilt every render; the caller's key says when it changed.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key, set]);
}
