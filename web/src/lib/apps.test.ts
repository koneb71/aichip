import { describe, expect, it } from "vitest";
import type { App, AppDetail, AppField, AppModel, AppView } from "./api";
import {
  appState,
  baseType,
  bucketValue,
  cellText,
  fieldLabel,
  formRows,
  isPaged,
  listColumns,
  pageWindow,
  recordFor,
  searchableField,
  searchFilter,
  sortParam,
  ungrantedScopes,
} from "./apps";

const field = (over: Partial<AppField> & { name: string }): AppField => ({
  label: null,
  type: "text",
  required: false,
  computed: false,
  hasDefault: false,
  ...over,
});

const MODEL: AppModel = {
  name: "expense",
  fields: [
    field({ name: "note" }),
    field({ name: "amount", type: "decimal" }),
    field({ name: "qty", type: "int" }),
    field({ name: "paid", type: "bool" }),
    field({ name: "spent_on", type: "date" }),
    field({ name: "total", type: "decimal", computed: true }),
  ],
};

const view = (over: Partial<AppView>): AppView => ({
  name: "list",
  kind: "list",
  model: "expense",
  spec: {},
  ...over,
});

const app = (over: Partial<App> = {}): App => ({
  id: "a",
  projectId: "p",
  workspaceId: "w",
  slug: "expenses-abc123",
  name: "Expenses",
  icon: "▤",
  summary: "Track spending",
  brief: "",
  runtime: "module",
  active: true,
  path: "/tmp/apps/expenses-abc123",
  ...over,
});

describe("field labels", () => {
  it("reads an underscored name as a sentence, not a heading", () => {
    // "Spent On" looks like a column header for something else; "Spent on" is
    // the name of a thing.
    expect(fieldLabel(field({ name: "spent_on" }))).toBe("Spent on");
    expect(fieldLabel(field({ name: "note" }))).toBe("Note");
  });

  it("prefers a label the manifest gave", () => {
    expect(fieldLabel(field({ name: "spent_on", label: "Date" }))).toBe("Date");
  });
});

describe("cell text", () => {
  it("leaves a decimal exactly as it arrived", () => {
    // It is a string precisely so no digits are lost; formatting it would
    // convert to a double to do so and undo that.
    expect(cellText("12345678901234567890.12", "decimal")).toBe("12345678901234567890.12");
    expect(cellText("4.25", "decimal")).toBe("4.25");
  });

  it("says Yes and No rather than true and false", () => {
    expect(cellText(true, "bool")).toBe("Yes");
    expect(cellText(false, "bool")).toBe("No");
  });

  it("shows nothing for a value that is not there", () => {
    // Not "null", which is what String(null) would put in the cell.
    expect(cellText(null, "text")).toBe("");
    expect(cellText(undefined, "decimal")).toBe("");
    expect(cellText(null, "bool")).toBe("");
  });

  it("trims a date to its day", () => {
    expect(cellText("2026-08-02", "date")).toBe("2026-08-02");
    expect(cellText("2026-08-02T00:00:00Z", "date")).toBe("2026-08-02");
  });

  it("leaves a datetime it cannot read alone rather than showing Invalid Date", () => {
    expect(cellText("not a date", "datetime")).toBe("not a date");
  });

  it("treats a ref like the id it is", () => {
    expect(baseType("ref:order")).toBe("ref");
    expect(cellText("2b1c0e9a-0000-4000-8000-000000000000", "ref:order")).toBe(
      "2b1c0e9a-0000-4000-8000-000000000000",
    );
  });
});

describe("view layout", () => {
  it("falls back to every field when a list names no columns", () => {
    expect(listColumns(MODEL, view({}))).toEqual([
      "note", "amount", "qty", "paid", "spent_on", "total",
    ]);
    expect(listColumns(MODEL, view({ spec: { columns: ["note"] } }))).toEqual(["note"]);
    // An empty list is the same as none, not a table with no columns.
    expect(listColumns(MODEL, view({ spec: { columns: [] } })).length).toBe(6);
  });

  it("falls back to one group when a form declares none", () => {
    expect(formRows(MODEL, view({ kind: "form" }))).toEqual([
      ["note", "amount", "qty", "paid", "spent_on", "total"],
    ]);
    const grouped = view({ kind: "form", spec: { groups: [["note"], ["amount", "qty"]] } });
    expect(formRows(MODEL, grouped)).toEqual([["note"], ["amount", "qty"]]);
  });

  it("turns a declared sort into the order parameter", () => {
    expect(sortParam(view({}))).toBeUndefined();
    expect(sortParam(view({ spec: { sort: { field: "spent_on", descending: true } } })))
      .toBe("-spent_on");
    expect(sortParam(view({ spec: { sort: { field: "note", descending: false } } })))
      .toBe("note");
  });
});

describe("records for the expression language", () => {
  it("turns a decimal back into a number before anything compares it", () => {
    // The bug this prevents: comparing text, where "9" > "100".
    const r = recordFor(MODEL, { amount: "100", qty: 2 });
    expect(r.amount).toBe(100);
    expect(typeof r.amount).toBe("number");
    expect(r.qty).toBe(2);
  });

  it("keeps a missing value as null rather than dropping the key", () => {
    const r = recordFor(MODEL, { note: null, amount: null });
    expect(r).toEqual({ note: null, amount: null });
    expect("note" in r).toBe(true);
  });

  it("does not turn an unreadable decimal into NaN", () => {
    expect(recordFor(MODEL, { amount: "not a number" }).amount).toBeNull();
  });
});

const detail = (over: Partial<AppDetail>): AppDetail => ({
  ...app(),
  manifest: "name: T",
  pending: null,
  ...over,
});

describe("what a tile says", () => {
  it("leads with a broken manifest, because nothing else can be fixed first", () => {
    expect(appState(app(), detail({ manifestError: "models.x: bad" })).kind).toBe("broken");
    // Even when it is also switched off and also has a plan waiting.
    const both = detail({ manifestError: "bad", pending: { id: "p", statements: [] } });
    expect(appState(app({ active: false }), both).kind).toBe("broken");
  });

  it("counts only the destructive statements in a waiting plan", () => {
    // The safe ones are why the plan exists at all; they are not what is
    // being asked about.
    const state = appState(
      app(),
      detail({
        pending: {
          id: "p",
          statements: [
            { sql: "CREATE SCHEMA x", destructive: false, why: "" },
            { sql: "ALTER TABLE t DROP COLUMN a", destructive: true, why: "" },
          ],
        },
      }),
    );
    expect(state.kind).toBe("attention");
    expect(state.line).toBe("A change to its tables is waiting for you.");
  });

  it("says an inactive app still has its data", () => {
    // The whole reason the switch is safe to use.
    const state = appState(app({ active: false }), null);
    expect(state.kind).toBe("off");
    expect(state.line).toContain("still here");
  });

  it("falls back to something rather than an empty line", () => {
    expect(appState(app({ summary: "" }), null).line).toBe("Ready.");
  });
});

describe("scopes", () => {
  it("asks about what is missing, in a stable order", () => {
    expect(ungrantedScopes(["write:board", "read:board"], ["read:board"]))
      .toEqual(["write:board"]);
    expect(ungrantedScopes(["b", "a", "b"], [])).toEqual(["a", "b"]);
    expect(ungrantedScopes(["read:board"], ["read:board"])).toEqual([]);
  });
});

describe("searching", () => {
  it("searches the first text column", () => {
    expect(searchableField(MODEL)).toBe("note");
  });

  it("offers no box when there is nothing text to search", () => {
    // Better than a box that could never match anything.
    const numbers: AppModel = {
      name: "reading",
      fields: [field({ name: "value", type: "decimal" }), field({ name: "at", type: "datetime" })],
    };
    expect(searchableField(numbers)).toBeNull();
  });

  it("builds a filter the query layer parses, and nothing when empty", () => {
    expect(searchFilter("note", "rent")).toEqual(["note:like:rent"]);
    expect(searchFilter("note", "  ")).toEqual([]);
    expect(searchFilter(null, "rent")).toEqual([]);
    // Not string-built: a term with a wildcard in it is handed over as-is and
    // apps::query does the escaping.
    expect(searchFilter("note", "50%")).toEqual(["note:like:50%"]);
  });
});

describe("paging", () => {
  it("pages a list but never a board", () => {
    // A per-column count that silently means "on this page" is a number that
    // lies, and adding the columns up is how someone finds out.
    expect(isPaged("list")).toBe(true);
    expect(isPaged("form")).toBe(true);
    expect(isPaged("kanban")).toBe(false);
    expect(isPaged("chart")).toBe(false);
  });

  it("counts from one and stops at the total", () => {
    expect(pageWindow(0, 60, 50)).toMatchObject({ from: 1, to: 50, hasPrevious: false, hasNext: true });
    expect(pageWindow(1, 60, 50)).toMatchObject({ from: 51, to: 60, hasPrevious: true, hasNext: false });
  });

  it("does not offer a pager that would always say the same thing", () => {
    expect(pageWindow(0, 12, 50).needed).toBe(false);
    expect(pageWindow(0, 50, 50).needed).toBe(false);
    expect(pageWindow(0, 51, 50).needed).toBe(true);
  });

  it("says nothing rather than 1–0 of 0 when there is nothing", () => {
    expect(pageWindow(0, 0, 50)).toMatchObject({ from: 0, to: 0, hasNext: false, needed: false });
  });
});

describe("chart buckets", () => {
  it("reads the text the server sent as a number", () => {
    expect(bucketValue("12.5")).toBe(12.5);
    expect(bucketValue(null)).toBe(0);
    // A sum over no rows comes back empty rather than as zero.
    expect(bucketValue("")).toBe(0);
    expect(bucketValue("not a number")).toBe(0);
  });
});
