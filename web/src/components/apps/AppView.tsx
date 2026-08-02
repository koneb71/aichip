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
  listColumns,
  recordFor,
  sortParam,
} from "../../lib/apps";
import { showIf } from "../../lib/expr";
import { FieldInput } from "./FieldInput";

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
}: {
  app: AppDetail;
  manifest: AppManifest;
  view: ViewDecl;
  /** What the menu calls this screen. The view's own name is an identifier —
   *  showing "list" beside a tab that already says "Expenses" is noise. */
  title?: string;
}) {
  const model = manifest.models.find((m) => m.name === view.model);
  const [rows, setRows] = useState<AppRow[]>([]);
  const [total, setTotal] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [editing, setEditing] = useState<AppRow | "new" | null>(null);
  const [buckets, setBuckets] = useState<{ bucket: string | null; value: string | null }[]>([]);

  const refresh = useCallback(async () => {
    if (!model) return;
    try {
      if (view.kind === "chart") {
        setBuckets((await api.appChart(app.id, view.name)).buckets);
      } else {
        const r = await api.appRows(app.id, model.name, { order: sortParam(view), limit: 200 });
        setRows(r.rows);
        setTotal(r.total);
      }
      setError(null);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    }
  }, [app.id, model, view]);

  useEffect(() => {
    refresh();
  }, [refresh]);

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
        <KanbanView model={model} view={view} rows={rows} onOpen={setEditing} />
      )}
      {view.kind === "chart" && <ChartView view={view} buckets={buckets} />}

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
}: {
  model: AppModel;
  view: ViewDecl;
  rows: AppRow[];
  onOpen: (row: AppRow) => void;
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
  const max = Math.max(1, ...values);
  if (buckets.length === 0) return <Empty>Nothing to chart yet.</Empty>;

  // Bars for every shape in this first pass. A pie that is really a bar chart
  // is legible and honest; a half-drawn pie is neither.
  return (
    <div className="rounded-xl border border-line p-4">
      <div className="mb-3 text-[11px] uppercase tracking-wide text-ink-dim">
        {view.spec.measure} by {view.spec.groupBy}
      </div>
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
            {/* The server's text, not the number it was parsed into, so a
                summed decimal shows every digit it actually has. */}
            <div className="w-28 shrink-0 text-right tabular-nums">{b.value ?? "—"}</div>
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
}: {
  app: AppDetail;
  manifest: AppManifest;
  model: AppModel;
  view: ViewDecl;
  row: AppRow | null;
  onClose: () => void;
  onSaved: () => void;
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
      // A step that deleted or changed the record makes what is on screen
      // stale, so the list is what should be looked at next.
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
