/**
 * Colours for the code surfaces — the one place raw hex may live.
 *
 * Monaco and xterm paint into a canvas or their own DOM and take colours as
 * strings in a theme object, so they cannot read a CSS variable the way a
 * Tailwind utility does. Everything around them (the Files tab's explorer,
 * tabs, status bar; the terminal's header) is ordinary token-painted chrome;
 * only what these two libraries draw is spelled out here.
 *
 * The surface colours mirror the tokens in index.css (`--color-panel`,
 * `--color-panel-2`, `--color-fg`, `--color-fg-subtle`…) for each theme, so the
 * editor and the terminal sit flush with the chrome that frames them. Change a
 * token there and the matching value here should follow. The ANSI and syntax
 * colours are VS Code's, tuned per theme for contrast on that background.
 */

/** The two app themes, as `lib/theme` resolves them. */
export type CodeTheme = "light" | "dark";

/** Monaco's theme names, registered by `components/editor/CodeEditor`. */
export const MONACO_THEME_NAME: Record<CodeTheme, string> = {
  light: "aichip",
  dark: "aichip-dark",
};

/** Monaco's colour overrides, layered on `vs` / `vs-dark`. */
export const MONACO_COLORS: Record<CodeTheme, Record<string, string>> = {
  light: {
    // --color-panel
    "editor.background": "#ffffff",
    "editorLineNumber.foreground": "#9ca3af",
    "editorLineNumber.activeForeground": "#4b5563",
    "editor.lineHighlightBackground": "#f6f6f7",
    "editorIndentGuide.background1": "#ececee",
  },
  dark: {
    // --color-panel (dark)
    "editor.background": "#141519",
    "editorGutter.background": "#141519",
    "minimap.background": "#141519",
    // --color-fg-subtle (dark)
    "editorLineNumber.foreground": "#6c6c78",
    "editorLineNumber.activeForeground": "#cccccc",
    // --color-panel-2 (dark)
    "editor.lineHighlightBackground": "#1b1c21",
    "editorIndentGuide.background1": "#26272e",
  },
};

/** The slice of xterm's `ITheme` set here — spelled out rather than imported,
 *  so this module stays free of xterm (TerminalPanel is its only importer). */
export interface TerminalPalette {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  black: string;
  red: string;
  green: string;
  yellow: string;
  blue: string;
  magenta: string;
  cyan: string;
  white: string;
  brightBlack: string;
  brightRed: string;
  brightGreen: string;
  brightYellow: string;
  brightBlue: string;
  brightMagenta: string;
  brightCyan: string;
  brightWhite: string;
}

/** xterm's palette for each theme. Background matches `--color-panel`. */
export const TERMINAL_THEME: Record<CodeTheme, TerminalPalette> = {
  light: {
    background: "#ffffff",
    foreground: "#17171c",
    cursor: "#17171c",
    cursorAccent: "#ffffff",
    selectionBackground: "#c9cbf3",
    black: "#000000",
    red: "#cd3131",
    green: "#107c10",
    yellow: "#8a6d00",
    blue: "#0451a5",
    magenta: "#bc05bc",
    cyan: "#0e7490",
    white: "#5f5f6b",
    brightBlack: "#666666",
    brightRed: "#cd3131",
    brightGreen: "#14801e",
    brightYellow: "#8a6d00",
    brightBlue: "#0451a5",
    brightMagenta: "#bc05bc",
    brightCyan: "#0e7490",
    brightWhite: "#8e8e99",
  },
  dark: {
    background: "#141519",
    foreground: "#cccccc",
    cursor: "#cccccc",
    cursorAccent: "#141519",
    selectionBackground: "#264f78",
    black: "#000000",
    red: "#f48771",
    green: "#89d185",
    yellow: "#e2c08d",
    blue: "#569cd6",
    magenta: "#c586c0",
    cyan: "#4ec9b0",
    white: "#cccccc",
    brightBlack: "#6e7681",
    brightRed: "#f48771",
    brightGreen: "#89d185",
    brightYellow: "#e2c08d",
    brightBlue: "#569cd6",
    brightMagenta: "#c586c0",
    brightCyan: "#4ec9b0",
    brightWhite: "#ffffff",
  },
};
