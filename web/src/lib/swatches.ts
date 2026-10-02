/**
 * Agent colours.
 *
 * These are data, not styling: a person picks one, the server stores it, and
 * every avatar, roster and chart draws with it. That is why they are raw hex —
 * a token would re-theme a choice someone made — and why this is the one place
 * outside the code-surface themes where hex may live (lib/design-scan.test.ts).
 */

/** What the agent editor offers. */
export const AGENT_SWATCHES = ["#4f46e5", "#059669", "#c026d3", "#ea580c", "#0284c7", "#dc2626"];

/** What a screen reader calls each one: a swatch is otherwise an unnamed button. */
export const AGENT_SWATCH_NAMES: Record<string, string> = {
  "#4f46e5": "Indigo",
  "#059669": "Green",
  "#c026d3": "Magenta",
  "#ea580c": "Orange",
  "#0284c7": "Blue",
  "#dc2626": "Red",
};

/** A new agent's colour until someone picks another. */
export const DEFAULT_AGENT_COLOR = "#4f46e5";

/**
 * For a face whose agent never chose a colour: derived from the name, so the
 * same agent looks the same everywhere without anyone choosing.
 */
export const AVATAR_FALLBACK = ["#4f54d6", "#7c4ddb", "#0a72b5", "#0b7a55", "#b25f05", "#cc2a4a", "#5c6676"];
