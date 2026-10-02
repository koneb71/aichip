import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";

// Read from disk rather than imported: the Tailwind plugin rewrites CSS imports,
// `?raw` included, and the test is about the file as written.
const css = readFileSync(new URL("../index.css", import.meta.url), "utf8");

// Every text colour the design system offers, held to WCAG AA (4.5:1) against
// every surface it is meant to sit on — in both themes. It reads index.css
// itself, so a palette tweak that quietly makes a label unreadable in dark
// mode fails here rather than in someone's eyes.

function block(selector: string): Record<string, string> {
  const start = css.indexOf(selector);
  if (start < 0) throw new Error(`no ${selector} block in index.css`);
  const open = css.indexOf("{", start);
  let depth = 0;
  let end = open;
  for (; end < css.length; end++) {
    if (css[end] === "{") depth++;
    if (css[end] === "}" && --depth === 0) break;
  }
  const vars: Record<string, string> = {};
  for (const m of css.slice(open + 1, end).matchAll(/--([\w-]+):\s*([^;]+);/g)) {
    vars[m[1]] = m[2].trim();
  }
  return vars;
}

const light = block("@theme {");
const dark = { ...light, ...block(':root[data-theme="dark"] {') };

function resolve(vars: Record<string, string>, name: string, seen = 0): string {
  const v = vars[name];
  if (v === undefined) throw new Error(`token --${name} is not defined`);
  const ref = v.match(/^var\(--([\w-]+)\)$/);
  if (ref && seen < 8) return resolve(vars, ref[1], seen + 1);
  return v;
}

function luminance(hex: string): number {
  const m = hex.match(/^#([0-9a-f]{6})$/i);
  if (!m) throw new Error(`not a 6-digit hex colour: ${hex}`);
  const [r, g, b] = [0, 2, 4].map((i) => {
    const c = parseInt(m[1].slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

export function contrast(a: string, b: string): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

// [text token, surfaces it is used on]
const PAIRS: [string, string[]][] = [
  ["fg", ["bg", "panel", "panel-2", "raised"]],
  ["fg-muted", ["bg", "panel", "panel-2"]],
  ["accent-fg", ["bg", "panel", "panel-2", "accent-subtle"]],
  ["on-accent", ["accent"]],
  ["danger", ["panel"]],
  ["success-fg", ["success-subtle", "panel"]],
  ["warning-fg", ["warning-subtle", "panel"]],
  ["danger-fg", ["danger-subtle", "panel"]],
  ["info-fg", ["info-subtle", "panel"]],
  ["tier-easy", ["tier-easy-soft", "panel"]],
  ["tier-medium", ["tier-medium-soft", "panel"]],
  ["tier-complex", ["tier-complex-soft", "panel"]],
  ...(["indigo", "violet", "sky", "mint", "amber", "rose", "slate"] as const).map(
    (hue) => [`ink-${hue}`, [`tint-${hue}`]] as [string, string[]],
  ),
];

describe.each([
  ["light", light],
  ["dark", dark],
] as const)("%s theme", (_, vars) => {
  for (const [text, surfaces] of PAIRS) {
    for (const surface of surfaces) {
      it(`${text} on ${surface} is readable`, () => {
        const ratio = contrast(resolve(vars, `color-${text}`), resolve(vars, `color-${surface}`));
        expect(ratio, `${text} on ${surface}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
      });
    }
  }

  it("a border can be told from the surface it outlines", () => {
    // Non-text UI needs 3:1 against what is beside it only when it carries
    // meaning; a hairline only needs to be visible. 1.2 is the floor below
    // which it disappears on most panels.
    const ratio = contrast(resolve(vars, "color-border-strong"), resolve(vars, "color-panel"));
    expect(ratio).toBeGreaterThanOrEqual(1.2);
  });
});
