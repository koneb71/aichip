/**
 * The parts of drawing an app that are decisions rather than markup.
 *
 * Extracted so they can be tested: vitest here has no DOM, so anything that
 * lives inside a component is untestable by construction. What is in this file
 * is the branch matrix — which is where the bugs actually are.
 */

import type {
  App,
  AppDetail,
  AppField,
  AppModel,
  AppRow,
  AppView,
  ContainerState,
} from "./api";

/** What a field is called on screen. */
export function fieldLabel(field: AppField): string {
  if (field.label) return field.label;
  // `spent_on` reads as "Spent on". Not title case: a label of "Spent On" looks
  // like a header rather than a name for a thing.
  const words = field.name.replace(/_/g, " ").trim();
  return words.charAt(0).toUpperCase() + words.slice(1);
}

/** The base type of a field, with `ref:` stripped off. */
export function baseType(type: string): string {
  return type.startsWith("ref:") ? "ref" : type;
}

/**
 * A stored value as text for a cell.
 *
 * Deliberately not locale-formatted for numbers: a decimal arrives as a string
 * so it keeps every digit, and running it through `toLocaleString` would
 * convert it to a double to do so, undoing the whole point.
 */
export function cellText(value: unknown, type: string): string {
  if (value === null || value === undefined) return "";
  switch (baseType(type)) {
    case "bool":
      return value ? "Yes" : "No";
    case "date":
      return String(value).slice(0, 10);
    case "datetime": {
      const d = new Date(String(value));
      return Number.isNaN(d.getTime()) ? String(value) : d.toLocaleString();
    }
    case "json":
      return typeof value === "string" ? value : JSON.stringify(value);
    default:
      return String(value);
  }
}

/** Which fields a form should offer, in declaration order. */
export function formRows(model: AppModel, view: AppView): string[][] {
  const groups = view.spec.groups?.filter((g) => g.length > 0);
  if (groups && groups.length > 0) return groups;
  return [model.fields.map((f) => f.name)];
}

/** The columns a list shows, falling back to every declared field. */
export function listColumns(model: AppModel, view: AppView): string[] {
  const columns = view.spec.columns?.filter((c) => c.length > 0);
  return columns && columns.length > 0 ? columns : model.fields.map((f) => f.name);
}

/** The `order` parameter for a view's declared sort. */
export function sortParam(view: AppView): string | undefined {
  const sort = view.spec.sort;
  if (!sort) return undefined;
  return sort.descending ? `-${sort.field}` : sort.field;
}

/**
 * A row's values, ready for the expression language.
 *
 * Decimals come back as strings so no digits are lost on the wire, and have to
 * become numbers again before anything compares them — otherwise
 * `amount > 100` compares text and "9" beats "100".
 */
export function recordFor(model: AppModel, row: AppRow): Record<string, string | number | boolean | null> {
  const out: Record<string, string | number | boolean | null> = {};
  for (const [key, value] of Object.entries(row)) {
    const type = model.fields.find((f) => f.name === key)?.type;
    if (value === null || value === undefined) {
      out[key] = null;
    } else if (baseType(type ?? "text") === "decimal" && typeof value === "string") {
      const n = Number(value);
      out[key] = Number.isFinite(n) ? n : null;
    } else if (typeof value === "object") {
      out[key] = JSON.stringify(value);
    } else {
      out[key] = value as string | number | boolean;
    }
  }
  return out;
}

/** What an app's tile says it is doing, and whether that needs attention. */
export type AppState =
  | { kind: "ready"; line: string }
  | { kind: "off"; line: string }
  | { kind: "attention"; line: string }
  | { kind: "broken"; line: string };

/**
 * One line describing where an app stands.
 *
 * Ordered by what a person needs to know first: a broken manifest before a
 * waiting migration, a waiting migration before being switched off. An app can
 * be several of these at once and the tile has room for one.
 */
export function appState(app: App, detail?: AppDetail | null): AppState {
  if (detail?.manifestError) {
    return { kind: "broken", line: "This app's manifest has an error." };
  }
  if (detail?.pending) {
    const destructive = detail.pending.statements.filter((s) => s.destructive).length;
    return {
      kind: "attention",
      line:
        destructive === 1
          ? "A change to its tables is waiting for you."
          : `${destructive} changes to its tables are waiting for you.`,
    };
  }
  if (!app.active) return { kind: "off", line: "Switched off. Its data is still here." };
  if (app.runtime !== "module") return { kind: "ready", line: "Runs in a container." };
  return { kind: "ready", line: app.summary || "Ready." };
}

/**
 * Which of an app's requested scopes it has not been granted.
 *
 * Sorted, so the same manifest asks the same question in the same order twice
 * running rather than in whatever order the arrays happened to be in.
 */
export function ungrantedScopes(requested: string[], granted: string[]): string[] {
  return [...new Set(requested.filter((s) => !granted.includes(s)))].sort();
}

/** A number for a chart bar, from the text the server sent. */
export function bucketValue(value: string | null): number {
  if (value === null) return 0;
  const n = Number(value);
  return Number.isFinite(n) ? n : 0;
}

/**
 * Which field a search box searches.
 *
 * The first text column, because that is what "search" means to someone
 * looking at a table of things with names. A model with no text at all gets no
 * box, rather than one that could never match.
 */
export function searchableField(model: AppModel): string | null {
  return model.fields.find((f) => baseType(f.type) === "text")?.name ?? null;
}

/**
 * The filter a search term becomes.
 *
 * Always `like`, never string-building: `apps::query` takes `field:op:value`
 * and does the escaping, so a term containing `%` or a quote is a term.
 */
export function searchFilter(field: string | null, term: string): string[] {
  const trimmed = term.trim();
  return field && trimmed ? [`${field}:like:${trimmed}`] : [];
}

/**
 * Whether a view pages.
 *
 * A list does; a board does not. Splitting a kanban across pages breaks the
 * one thing it is for, and a per-column count that silently means "on this
 * page" is a number that lies.
 */
export function isPaged(kind: string): boolean {
  return kind === "list" || kind === "form";
}

export interface PageWindow {
  /** 1-indexed, inclusive, for display. */
  from: number;
  to: number;
  total: number;
  hasPrevious: boolean;
  hasNext: boolean;
  /** Whether a pager is worth showing at all. */
  needed: boolean;
}

/** What the pager says and which of its buttons work. */
export function pageWindow(page: number, total: number, size: number): PageWindow {
  const from = total === 0 ? 0 : page * size + 1;
  const to = Math.min((page + 1) * size, total);
  return {
    from,
    to,
    total,
    hasPrevious: page > 0,
    hasNext: (page + 1) * size < total,
    needed: total > size,
  };
}

/**
 * Where a container app is served.
 *
 * Built in the browser rather than by the server, for the same reason
 * `previewUrl` is: the port to put in it is the one aichip is being *served*
 * on, and the browser is the only party that already knows it.
 */
export function appOrigin(slug: string, port: string): string {
  return `http://${slug}.app.localhost${port ? `:${port}` : ""}`;
}

/** One line saying where a container app stands. */
export function containerLine(state: ContainerState | null): string {
  if (!state) return "Checking…";
  if (state.docker.usable === false) return "Docker isn't available.";
  switch (state.preview?.status) {
    case "running":
      return "Running at";
    case "building":
      return "Building — this takes a minute the first time.";
    case "idle":
      // The distinction that makes the button honest: waking is seconds
      // because the image is still here, and rebuilding is not.
      return "Asleep. Waking it takes a few seconds.";
    case "failed":
      return "The last build failed.";
    case "stopped":
      return "Stopped.";
    default:
      return "Not built yet.";
  }
}
