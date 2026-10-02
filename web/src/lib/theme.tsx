import { createContext, useCallback, useContext, useEffect, useMemo, useState, type ReactNode } from "react";

/**
 * Light, dark, or whatever the system says.
 *
 * The attribute on <html> is the whole mechanism: every colour is a CSS
 * variable that `:root[data-theme="dark"]` redefines, so switching is one
 * attribute write and nothing re-renders to recolour. The inline script in
 * index.html sets it before first paint from the same storage key, so a dark
 * reload never flashes white.
 *
 * The choice is a per-viewer convenience, so it lives in localStorage — and
 * every read and write is guarded, because a private window or blocked site
 * data throws, and a theme preference is not worth a blank page.
 */

export type ThemeChoice = "system" | "light" | "dark";
export type Theme = "light" | "dark";

export const THEME_KEY = "eren.theme";

export function readChoice(): ThemeChoice {
  try {
    const v = window.localStorage.getItem(THEME_KEY);
    return v === "light" || v === "dark" || v === "system" ? v : "system";
  } catch {
    return "system";
  }
}

function writeChoice(choice: ThemeChoice) {
  try {
    window.localStorage.setItem(THEME_KEY, choice);
  } catch {
    // Not remembered across reloads; the switch still works for this visit.
  }
}

/** What a choice means given the system preference. Pure, for the tests. */
export function resolveTheme(choice: ThemeChoice, systemDark: boolean): Theme {
  if (choice === "system") return systemDark ? "dark" : "light";
  return choice;
}

/** The next choice when the toggle is pressed: system → light → dark → system. */
export function nextChoice(choice: ThemeChoice): ThemeChoice {
  return choice === "system" ? "light" : choice === "light" ? "dark" : "system";
}

const QUERY = "(prefers-color-scheme: dark)";

function systemPrefersDark(): boolean {
  return typeof window !== "undefined" && !!window.matchMedia?.(QUERY).matches;
}

function apply(theme: Theme) {
  document.documentElement.dataset.theme = theme;
}

interface ThemeState {
  choice: ThemeChoice;
  theme: Theme;
  setChoice: (c: ThemeChoice) => void;
  cycle: () => void;
}

const Ctx = createContext<ThemeState | null>(null);

export function ThemeProvider({ children }: { children: ReactNode }) {
  const [choice, setChoiceState] = useState<ThemeChoice>(readChoice);
  const [systemDark, setSystemDark] = useState(systemPrefersDark);

  useEffect(() => {
    const mq = window.matchMedia?.(QUERY);
    if (!mq) return;
    const on = (e: MediaQueryListEvent) => setSystemDark(e.matches);
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, []);

  const theme = resolveTheme(choice, systemDark);
  useEffect(() => apply(theme), [theme]);

  const setChoice = useCallback((c: ThemeChoice) => {
    setChoiceState(c);
    writeChoice(c);
  }, []);
  const cycle = useCallback(() => setChoice(nextChoice(choice)), [choice, setChoice]);

  const value = useMemo(() => ({ choice, theme, setChoice, cycle }), [choice, theme, setChoice, cycle]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useTheme(): ThemeState {
  const v = useContext(Ctx);
  if (!v) throw new Error("useTheme outside ThemeProvider");
  return v;
}
