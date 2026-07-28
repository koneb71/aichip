import { useEffect, useState } from "react";

/**
 * Subscribe to a CSS media query from React.
 *
 * Layout that only needs to *look* different belongs in Tailwind breakpoints.
 * This is for the cases where a narrow viewport changes what is *rendered* —
 * three side-by-side panels becoming one pane behind a tab bar, say, where
 * hiding the other two with CSS would leave their scroll containers and
 * polling animations alive off-screen.
 */
export function useMediaQuery(query: string): boolean {
  const [matches, setMatches] = useState(
    () => typeof window !== "undefined" && window.matchMedia(query).matches,
  );

  useEffect(() => {
    const list = window.matchMedia(query);
    // Re-read on subscribe: the query may have changed, or the viewport may
    // have moved between the initial render and this effect.
    setMatches(list.matches);
    const onChange = (e: MediaQueryListEvent) => setMatches(e.matches);
    list.addEventListener("change", onChange);
    return () => list.removeEventListener("change", onChange);
  }, [query]);

  return matches;
}

/** Tailwind's `lg` breakpoint. Below this, side-by-side panels stop fitting. */
export const NARROW = "(max-width: 1023px)";
