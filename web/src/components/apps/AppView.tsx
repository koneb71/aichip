import { useCallback, useEffect, useMemo, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import {
  api,
  type AppDetail,
  type AppManifest,
  type AppModel,
  type AppRow,
  type AppView as ViewDecl,
} from "../../lib/api";
import {
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
} from "../../lib/apps";
import { showIf } from "../../lib/expr";
import { FieldInput } from "./FieldInput";

/** Rows per page. Big enough that most apps never see a pager. */
const PAGE = 50;

/**
 * How many rows a board fetches.
 *
 * A kanban is not paged, and that is deliberate: its whole point is seeing
 * everything at once, and a per-column count that silently means "on this
 * page" is a number that lies. So it asks for as much as the server will give
 * — see `MAX_LIMIT` in `apps::query` — and says so plainly when there is more.
 */
const BOARD = 1000;

/**
 * One of an app's screens, drawn from its declaration.
 *
 * The same four components render every app, which is what stops a gallery of
 * twelve apps looking like twelve products. Nothing here is app-specific: a
 * manifest says list-of-these-columns and this draws it.
 */
export function AppView({
  app,
  manifest,
  view,
  title,
  onGoto,
}: {
  app: AppDetail;
  manifest: AppManifest;
  view: ViewDecl;
  /** What the menu calls this screen. The view's own name is an identifier —
   *  showing "list" beside a tab that already says "Expenses" is noise. */
  title?: string;
  /** Where a `goto` step sends you. The page owns which screen is showing. */
  onGoto?: (view: string) => void;
}) {
  const model = manifest.models.find((m) => m.name === view.model);
  const [rows, setRows] = useState<AppRow[]>([]);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<AppRow | "new" | null>(null);
  const [buckets, setBuckets] = useState<{ bucket: string | null; value: string | null }[]>([]);
  const [page, setPage] = useState(0);
  const [search, setSearch] = useState("");

  const searchable = model ? searchableField(model) : null;
  const filters = useMemo(() => searchFilter(searchable, search), [search, searchable]);
  const paged = isPaged(view.kind);
  const window = pageWindow(page, total, PAGE);

  const refresh = useCallback(async () => {
    if (!model) return;
    try {
      if (view.kind === "chart") {
        setBuckets((await api.appChart(app.id, view.name, filters)).buckets);
      } else {
        const r = await api.appRows(app.id, model.name, {
          where: filters,
          order: sortParam(view),
          limit: paged ? PAGE : BOARD,
          offset: paged ? page * PAGE : 0,
        });
        setRows(r.rows);
        setTotal(r.total);
      }
      setError(null);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  }, [app.id, model, view, page, filters, paged]);

  // Debounced, because every keystroke is a query. 200ms is under the point a
  // person notices and well over the point the server does.
  useEffect(() => {
    const t = setTimeout(refresh, search ? 200 : 0);
    return () => clearTimeout(t);
  }, [refresh, search]);

  // Page 4 of a filter that now matches two rows is an empty screen with no
  // explanation, so narrowing the search goes back to the start.
  useEffect(() => {
    setPage(0);
  }, [search, view.name]);

  if (!model) {
    return <Empty>This view names a model the app no longer declares.</Empty>;
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      <div className="mb-3 flex items-center gap-3">
        {title && <h2 className="text-sm font-semibold">{title}</h2>}
        {view.kind !== "chart" && (
          <span className="text-xs text-ink-dim">
            {total} {total === 1 ? "row" : "rows"}
          </span>
        )}
        <div className="flex-1" />
        {searchable && (
          <input
            value={search}
            onChange={(e) => setSearch(e.target.value)}
            placeholder={`Search ${fieldLabel(model.fields.find((f) => f.name === searchable)!).toLowerCase()}…`}
            className="w-48 rounded-lg border border-line bg-surface px-2 py-1 text-xs outline-none focus:border-accent"
          />
        )}
        {view.kind !== "chart" && (
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => setEditing("new")}
            className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
          >
            Add
          </motion.button>
        )}
      </div>

      {error && (
        <div className="mb-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      {view.kind === "list" && (
        <ListView model={model} view={view} rows={rows} onOpen={setEditing} />
      )}
      {view.kind === "form" && (
        <ListView model={model} view={view} rows={rows} onOpen={setEditing} />
      )}
      {view.kind === "kanban" && (
        <KanbanView
          model={model}
          view={view}
          rows={rows}
          onOpen={setEditing}
          // Said out loud rather than left to be noticed by adding up the
          // column counts and finding they do not reach the total.
          truncated={total > rows.length ? total : null}
        />
      )}
      {view.kind === "chart" && <ChartView view={view} buckets={buckets} />}

      {/* Only when there is a second page. A pager that always says "1 of 1"
          is furniture. */}
      {paged && window.needed && (
        <div className="mt-3 flex items-center gap-2 text-xs">
          <button
            onClick={() => setPage((p) => Math.max(0, p - 1))}
            disabled={!window.hasPrevious}
            className="rounded-lg border border-line px-2 py-1 hover:bg-line/40 disabled:opacity-40"
          >
            Previous
          </button>
          <span className="text-ink-dim">
            {window.from}–{window.to} of {window.total}
          </span>
          <button
            onClick={() => setPage((p) => p + 1)}
            disabled={!window.hasNext}
            className="rounded-lg border border-line px-2 py-1 hover:bg-line/40 disabled:opacity-40"
          >
            Next
          </button>
        </div>
      )}

      <AnimatePresence>
        {editing && (
          <RowEditor
            app={app}
            manifest={manifest}
            model={model}
            view={view}
            row={editing === "new" ? null : editing}
            onClose={() => setEditing(null)}
            onSaved={() => {
              setEditing(null);
              refresh();
            }}
            onGoto={(to) => {
              setEditing(null);
              onGoto?.(to);
            }}
          />
        )}
      </AnimatePresence>
    </div>
  );
}

function Empty({ children }: { children: React.ReactNode }) {
  return (
    <div className="rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
      {children}
    </div>
  );
}

function ListView({
  model,
  view,
  rows,
  onOpen,
}: {
  model: AppModel;
  view: ViewDecl;
  rows: AppRow[];
  onOpen: (row: AppRow) => void;
}) {
  const columns = useMemo(() => listColumns(model, view), [model, view]);
  if (rows.length === 0) return <Empty>Nothing here yet.</Empty>;

  return (
    // The table scrolls inside its own box rather than pushing the page wide:
    // an app with twelve columns must not make the dashboard scroll sideways.
    <div className="min-h-0 flex-1 overflow-auto rounded-xl border border-line">
      <table className="w-full text-left text-xs">
        <thead className="sticky top-0 bg-panel-2">
          <tr>
            {columns.map((c) => {
              const f = model.fields.find((x) => x.name === c);
              return (
                <th key={c} className="whitespace-nowrap px-3 py-2 font-semibold text-ink-dim">
                  {f ? fieldLabel(f) : c}
                </th>
              );
            })}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr
              key={String(row.id)}
              onClick={() => onOpen(row)}
              className="cursor-pointer border-t border-line hover:bg-line/30"
            >
              {columns.map((c) => (
                <td key={c} className="px-3 py-2">
                  {cellText(row[c], model.fields.find((f) => f.name === c)?.type ?? "text")}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function KanbanView({
  model,
  view,
  rows,
  onOpen,
  truncated,
}: {
  model: AppModel;
  view: ViewDecl;
  rows: AppRow[];
  onOpen: (row: AppRow) => void;
  /** The real total, when there are more rows than the board is showing. */
  truncated: number | null;
}) {
  const groupBy = view.spec.groupBy ?? model.fields[0]?.name;
  const titleField = view.spec.title ?? model.fields[0]?.name;
  const columns = useMemo(() => {
    const out = new Map<string, AppRow[]>();
    for (const row of rows) {
      // Rows with nothing in the grouping field still belong somewhere, or
      // they simply vanish from the board and nobody knows why.
      const key = cellText(row[groupBy ?? ""], "text") || "—";
      (out.get(key) ?? out.set(key, []).get(key)!).push(row);
    }
    return [...out.entries()];
  }, [rows, groupBy]);

  if (rows.length === 0) return <Empty>Nothing here yet.</Empty>;

  return (
    <div className="min-h-0 flex-1 overflow-x-auto">
      {truncated !== null && (
        <div className="mb-2 rounded-lg bg-amber-50 px-3 py-1.5 text-xs text-amber-900">
          Showing the first {rows.length} of {truncated}. Narrow it with the search box —
          the counts below are for what is on the board, not for everything.
        </div>
      )}
      <div className="flex gap-3">
        {columns.map(([key, group]) => (
          <div key={key} className="w-64 shrink-0 rounded-xl bg-panel-2 p-2">
            <div className="mb-2 flex items-center justify-between px-1 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              <span>{key}</span>
              <span>{group.length}</span>
            </div>
            <div className="flex flex-col gap-2">
              {group.map((row) => (
                <button
                  key={String(row.id)}
                  onClick={() => onOpen(row)}
                  className="card-shadow rounded-lg bg-panel p-2 text-left text-xs hover:bg-line/30"
                >
                  <div className="font-medium">
                    {cellText(row[titleField ?? ""], "text") || "Untitled"}
                  </div>
                  {(view.spec.fields ?? []).map((f) => (
                    <div key={f} className="mt-1 text-ink-dim">
                      {cellText(row[f], model.fields.find((x) => x.name === f)?.type ?? "text")}
                    </div>
                  ))}
                </button>
              ))}
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}

function ChartView({
  view,
  buckets,
}: {
  view: ViewDecl;
  buckets: { bucket: string | null; value: string | null }[];
}) {
  const values = buckets.map((b) => bucketValue(b.value));
  if (buckets.length === 0) return <Empty>Nothing to chart yet.</Empty>;

  const shape = view.spec.shape ?? "bar";
  return (
    <div className="rounded-xl border border-line p-4">
      <div className="mb-3 text-[11px] uppercase tracking-wide text-ink-dim">
        {view.spec.measure} by {view.spec.groupBy}
      </div>
      {shape === "line" ? (
        <LineChart buckets={buckets} values={values} />
      ) : shape === "pie" ? (
        <PieChart buckets={buckets} values={values} />
      ) : (
        <BarChart buckets={buckets} values={values} />
      )}
    </div>
  );
}

/**
 * One colour per slice, borrowed from the tier variables the dashboard already
 * uses. Categorical rather than a gradient: these buckets are names, and a
 * gradient would imply an order they do not have.
 */
const SERIES = [
  "var(--color-accent)",
  "var(--color-tier-easy)",
  "var(--color-tier-complex)",
  "var(--color-tier-medium)",
  "var(--color-danger)",
];

type Bucket = { bucket: string | null; value: string | null };

function BarChart({ buckets, values }: { buckets: Bucket[]; values: number[] }) {
  const max = Math.max(1, ...values);
  return (
    <div className="flex flex-col gap-2">
      {buckets.map((b, i) => (
        <div key={`${b.bucket}-${i}`} className="flex items-center gap-2 text-xs">
          <div className="w-32 shrink-0 truncate text-ink-dim" title={b.bucket ?? ""}>
            {b.bucket || "—"}
          </div>
          <div className="h-4 min-w-0 flex-1 rounded bg-line/40">
            <div
              className="h-4 rounded"
              style={{
                width: `${Math.max(2, (values[i] / max) * 100)}%`,
                background: "var(--color-accent)",
              }}
            />
          </div>
          {/* The server's text, not the number it was parsed into, so a summed
              decimal shows every digit it actually has. */}
          <div className="w-28 shrink-0 text-right tabular-nums">{b.value ?? "—"}</div>
        </div>
      ))}
    </div>
  );
}

function LineChart({ buckets, values }: { buckets: Bucket[]; values: number[] }) {
  const W = 640;
  const H = 180;
  const PAD = 8;
  const max = Math.max(1, ...values);
  // A single point has no span to divide by, so it sits in the middle rather
  // than at x = NaN.
  const x = (i: number) =>
    buckets.length === 1 ? W / 2 : PAD + (i * (W - PAD * 2)) / (buckets.length - 1);
  const y = (v: number) => H - PAD - (v / max) * (H - PAD * 2);
  const points = values.map((v, i) => `${x(i)},${y(v)}`).join(" ");

  return (
    <div className="overflow-x-auto">
      <svg viewBox={`0 0 ${W} ${H}`} className="h-44 w-full min-w-[24rem]" role="img">
        <polyline
          points={points}
          fill="none"
          stroke="var(--color-accent)"
          strokeWidth={2}
          strokeLinejoin="round"
          strokeLinecap="round"
        />
        {values.map((v, i) => (
          <circle key={i} cx={x(i)} cy={y(v)} r={3} fill="var(--color-accent)">
            <title>{`${buckets[i].bucket ?? "—"}: ${buckets[i].value ?? ""}`}</title>
          </circle>
        ))}
      </svg>
      <div className="mt-1 flex justify-between text-[11px] text-ink-dim">
        <span className="truncate">{buckets[0]?.bucket || "—"}</span>
        {buckets.length > 1 && (
          <span className="truncate">{buckets[buckets.length - 1]?.bucket || "—"}</span>
        )}
      </div>
    </div>
  );
}

function PieChart({ buckets, values }: { buckets: Bucket[]; values: number[] }) {
  const total = values.reduce((a, b) => a + b, 0);
  if (total <= 0) return <Empty>Every slice is zero, so there is no pie to draw.</Empty>;

  const R = 70;
  let angle = -Math.PI / 2; // start at twelve o'clock, where a pie is read from
  const slices = values.map((v, i) => {
    const sweep = (v / total) * Math.PI * 2;
    const from = angle;
    angle += sweep;
    const p = (a: number) => `${(R + R * Math.cos(a)).toFixed(2)},${(R + R * Math.sin(a)).toFixed(2)}`;
    // A slice of the whole circle cannot be drawn as an arc — its start and
    // end points are the same, so the path collapses to nothing.
    const d =
      sweep >= Math.PI * 2 - 1e-9
        ? `M ${R},${R} m ${-R},0 a ${R},${R} 0 1,0 ${R * 2},0 a ${R},${R} 0 1,0 ${-R * 2},0`
        : `M ${R},${R} L ${p(from)} A ${R},${R} 0 ${sweep > Math.PI ? 1 : 0},1 ${p(angle)} Z`;
    return { d, colour: SERIES[i % SERIES.length] };
  });

  return (
    <div className="flex flex-wrap items-center gap-6">
      <svg viewBox={`0 0 ${R * 2} ${R * 2}`} className="h-40 w-40 shrink-0" role="img">
        {slices.map((s, i) => (
          <path key={i} d={s.d} fill={s.colour}>
            <title>{`${buckets[i].bucket ?? "—"}: ${buckets[i].value ?? ""}`}</title>
          </path>
        ))}
      </svg>
      <div className="flex min-w-0 flex-col gap-1 text-xs">
        {buckets.map((b, i) => (
          <div key={`${b.bucket}-${i}`} className="flex items-center gap-2">
            <span
              className="h-2.5 w-2.5 shrink-0 rounded-sm"
              style={{ background: SERIES[i % SERIES.length] }}
            />
            <span className="min-w-0 truncate text-ink-dim">{b.bucket || "—"}</span>
            <span className="tabular-nums">{b.value ?? "—"}</span>
            <span className="text-ink-dim">
              {((values[i] / total) * 100).toFixed(0)}%
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

function RowEditor({
  app,
  manifest,
  model,
  view,
  row,
  onClose,
  onSaved,
  onGoto,
}: {
  app: AppDetail;
  manifest: AppManifest;
  model: AppModel;
  view: ViewDecl;
  row: AppRow | null;
  onClose: () => void;
  onSaved: () => void;
  onGoto: (view: string) => void;
}) {
  const [draft, setDraft] = useState<AppRow>(row ?? {});
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [notices, setNotices] = useState<string[]>([]);
  const [needsScope, setNeedsScope] = useState<string | null>(null);

  const form = manifest.views.find((v) => v.kind === "form" && v.model === model.name) ?? view;
  const groups = formRows(model, form);
  const record = useMemo(() => recordFor(model, draft), [model, draft]);
  const now = new Date().toISOString();

  const save = async () => {
    setBusy(true);
    setError(null);
    try {
      // Only the fields a person can actually set. Computed ones are worked
      // out server-side on every save, and sending them back is refused.
      const values: AppRow = {};
      for (const f of model.fields) {
        if (f.computed) continue;
        if (f.name in draft) values[f.name] = draft[f.name];
      }
      if (row?.id) {
        await api.changeAppRow(app.id, model.name, String(row.id), values);
      } else {
        await api.addAppRow(app.id, model.name, values);
      }
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  /**
   * Press a declared button.
   *
   * A missing permission is not treated as a failure: it comes back as
   * `needsScope`, which is a thing the person can grant, so it gets its own
   * note rather than the red box errors use.
   */
  const press = async (name: string) => {
    if (!row?.id) return;
    setBusy(true);
    setError(null);
    setNeedsScope(null);
    setNotices([]);
    try {
      const out = await api.runAppAction(app.id, name, model.name, String(row.id));
      setNeedsScope(out.needsScope);
      setNotices(out.messages);
      // `goto` wins over everything else: the action asked to be somewhere
      // else, so staying on this record to read a notice would be ignoring it.
      if (out.goto) {
        onGoto(out.goto);
        return;
      }
      // A step that deleted or changed the record makes what is on screen
      // stale, so the list is what should be looked at next. A notice is the
      // one reason to stay — it is there to be read.
      if (out.deleted) onSaved();
      else if (out.messages.length === 0 && !out.needsScope) onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const remove = async () => {
    if (!row?.id) return;
    setBusy(true);
    try {
      await api.removeAppRow(app.id, model.name, String(row.id));
      onSaved();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      setBusy(false);
    }
  };

  return (
    <motion.div
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      exit={{ opacity: 0 }}
      onClick={onClose}
      className="fixed inset-0 z-40 flex items-center justify-center bg-black/30 p-4"
    >
      <motion.div
        initial={{ scale: 0.97, y: 8 }}
        animate={{ scale: 1, y: 0 }}
        exit={{ scale: 0.97, y: 8 }}
        transition={{ type: "spring", stiffness: 420, damping: 32 }}
        onClick={(e) => e.stopPropagation()}
        className="card-shadow max-h-[85vh] w-full max-w-lg overflow-auto rounded-2xl bg-panel p-5"
      >
        <h3 className="mb-4 text-sm font-semibold">
          {row ? "Edit" : "Add"} {model.name}
        </h3>

        <div className="flex flex-col gap-4">
          {groups.map((group, i) => (
            <div key={i} className="grid grid-cols-2 gap-3">
              {group.map((name) => {
                const field = model.fields.find((f) => f.name === name);
                if (!field) return null;
                return (
                  <FieldInput
                    key={name}
                    field={field}
                    value={draft[name]}
                    onChange={(v) => setDraft((d) => ({ ...d, [name]: v }))}
                  />
                );
              })}
            </div>
          ))}
        </div>

        {/* Buttons the form declares, hidden by their own condition. The
            expression runs here, in the browser, which is why there are two
            implementations of the language and one shared corpus. */}
        {(form.spec.buttons ?? []).length > 0 && (
          <div className="mt-4 flex flex-wrap items-center gap-2 border-t border-line pt-3">
            {(form.spec.buttons ?? []).map((name) => {
              const action = manifest.actions.find((a) => a.name === name);
              if (!action || !showIf(action.showIf, record, now)) return null;
              return (
                <motion.button
                  key={name}
                  whileTap={{ scale: 0.96 }}
                  onClick={() => press(action.name)}
                  disabled={busy || !row?.id}
                  title={row?.id ? undefined : "Save the record first."}
                  className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40 disabled:opacity-50"
                >
                  {action.label}
                </motion.button>
              );
            })}
          </div>
        )}

        {needsScope && (
          <div className="mt-3 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
            That button needs the <span className="font-mono">{needsScope}</span> permission,
            which this app does not have. Grant it under Permissions and try again.
          </div>
        )}
        {notices.map((m, i) => (
          <div key={i} className="mt-3 rounded-lg bg-panel-2 px-3 py-2 text-xs">
            {m}
          </div>
        ))}

        {error && (
          <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
        )}

        <div className="mt-5 flex items-center gap-2">
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={save}
            disabled={busy}
            className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
          >
            Save
          </motion.button>
          <button onClick={onClose} className="rounded-lg px-3 py-1.5 text-xs text-ink-dim">
            Cancel
          </button>
          <div className="flex-1" />
          {row?.id != null && (
            <button
              onClick={remove}
              disabled={busy}
              className="rounded-lg px-2 py-1.5 text-xs text-danger hover:bg-red-50"
            >
              Delete
            </button>
          )}
        </div>
      </motion.div>
    </motion.div>
  );
}
