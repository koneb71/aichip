import { useCallback, useEffect, useState } from "react";
import { Link } from "react-router-dom";
import { api, type App } from "../lib/api";
import { appState } from "../lib/apps";
import { useWorkspace } from "../lib/workspace";
import { NewAppModal } from "../components/apps/NewAppModal";
import { RepoApps } from "../components/apps/RepoApps";
import { Empty, Page, PageHead } from "../components/ui/Surface";
import { Icon } from "../components/ui/Icon";
import { Button, buttonClasses } from "../components/ui/Button";
import { Switch } from "../components/ui/Field";

/**
 * The gallery.
 *
 * Apps you install, switch on and use. A module renders here in the dashboard
 * and executes nothing; a container app is the escape hatch and says so.
 */
export default function AppsPage() {
  const { active } = useWorkspace();
  const [apps, setApps] = useState<App[]>([]);
  const [adding, setAdding] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!active) return;
    api
      .apps(active.id)
      .then((r) => setApps(r.apps))
      .catch(() => {});
  }, [active]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const toggle = async (app: App) => {
    // Optimistic: the switch should feel like a switch. A failure puts it back
    // and says why rather than leaving the UI ahead of the server.
    setApps((prev) => prev.map((a) => (a.id === app.id ? { ...a, active: !a.active } : a)));
    try {
      await api.setAppActive(app.id, !app.active);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
      refresh();
    }
  };

  return (
    <Page>
      <PageHead
        title="Apps"
        subtitle="Small internal tools Eren builds and hosts for you, each with its own data and screens."
        actions={
          <>
        <label className={buttonClasses({ variant: "secondary", className: "cursor-pointer" })}>
          Import
          <input
            type="file"
            accept=".erenapp,.json,application/json"
            className="hidden"
            onChange={async (e) => {
              const file = e.target.files?.[0];
              // Cleared straight away so picking the same file twice still
              // fires a change event — otherwise a failed import cannot be
              // retried without choosing something else first.
              e.target.value = "";
              if (!file || !active) return;
              setError(null);
              try {
                await api.importApp(active.id, await file.text());
                refresh();
              } catch (err) {
                setError(String(err).replace(/^Error:\s*/, ""));
              }
            }}
          />
        </label>
        <Button
          variant="primary"
          onClick={() => setAdding(true)}
          icon={<Icon name="plus" size={15} strokeWidth={2.5} />}
        >
          New app
        </Button>
          </>
        }
      />

      {error && (
        <div className="mb-4 rounded-lg bg-danger-subtle px-3 py-2 text-xs text-danger-fg">{error}</div>
      )}

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-2 xl:grid-cols-3">
        {apps.map((app) => {
          const state = appState(app, null);
          return (
            <div
              key={app.id}
              className="card-shadow lift group flex flex-col rounded-2xl border border-border bg-panel p-4"
              style={{ opacity: app.active ? 1 : 0.6 }}
            >
              <div className="flex items-start gap-3">
                <span className="text-xl leading-none">{app.icon}</span>
                <div className="min-w-0 flex-1">
                  <Link
                    to={`/apps/${app.id}`}
                    className="block truncate text-sm font-semibold hover:underline"
                  >
                    {app.name}
                  </Link>
                  <p className="mt-0.5 line-clamp-2 text-xs text-fg-muted">{state.line}</p>
                </div>
                {/* A switch, not a delete. Off keeps every row — which is the
                    whole reason it is safe to use, and worth saying on the
                    tile rather than only in a confirmation nobody reads. */}
                <span
                  title={app.active ? "Switch off. Keeps its data." : "Switch on"}
                  className="inline-flex shrink-0"
                >
                  <Switch
                    checked={app.active}
                    onChange={() => toggle(app)}
                    label={app.active ? "Switch off. Keeps its data." : "Switch on"}
                  />
                </span>
              </div>
              <div className="mt-3 flex items-center gap-2 text-[11px] text-fg-muted">
                <span className="rounded bg-panel-2 px-1.5 py-0.5">
                  {app.runtime === "module" ? "module" : `container · ${app.runtime}`}
                </span>
                <span className="truncate font-mono">{app.slug}</span>
              </div>
            </div>
          );
        })}

        {apps.length === 0 && (
          <div className="col-span-full">
            <Empty
              icon={<Icon name="apps" size={28} />}
              title="No apps yet"
              hint="Describe one and Eren will write the manifest, or paste one you already have. An app gets its own tables, screens and worktree."
            />
          </div>
        )}
      </div>

      {active && <RepoApps workspaceId={active.id} onSynced={refresh} />}

      {adding && active && (
        <NewAppModal
          workspaceId={active.id}
          onClose={() => setAdding(false)}
          onInstalled={() => {
            setAdding(false);
            refresh();
          }}
        />
      )}
    </Page>
  );
}
