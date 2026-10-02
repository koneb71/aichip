import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, type AppDetail } from "../lib/api";
import { AppView } from "../components/apps/AppView";
import { SchemaGate } from "../components/apps/SchemaGate";
import { ScopeGrant } from "../components/apps/ScopeGrant";
import { AppFrame } from "../components/apps/AppFrame";
import { screenPath } from "../lib/apps";
import { BuildHistory } from "../components/apps/BuildHistory";
import { ChangeAppModal } from "../components/apps/ChangeAppModal";
import { DockerfileGate } from "../components/apps/DockerfileGate";
import { springy } from "../lib/motion";
import { Button, buttonClasses } from "../components/ui/Button";
import { Textarea } from "../components/ui/Field";

/** One app: its screens, and the two things that can be wrong with it. */
export default function AppPage() {
  const { appId } = useParams();
  const navigate = useNavigate();
  const [params] = useSearchParams();
  const [app, setApp] = useState<AppDetail | null>(null);
  const [screen, setScreen] = useState<string | null>(null);
  const [editing, setEditing] = useState<string | null>(null);
  const [perms, setPerms] = useState(false);
  const [changing, setChanging] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  // The sidebar links straight to a screen, so `?view=` wins over the menu's
  // first entry — otherwise every link in it would land on the same page.
  const wanted = params.get("view");

  const refresh = useCallback(() => {
    if (!appId) return;
    api
      .app(appId)
      .then((a) => {
        setApp(a);
        // Only when nothing has chosen one yet — the effect below has already
        // applied `?view=` by the time this resolves.
        setScreen((s) => s ?? a.declares?.menu[0]?.view ?? a.declares?.views[0]?.name ?? null);
      })
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
    // Deliberately not keyed on `?view=`: a sidebar click changes the screen,
    // not the app, and refetching it on every one would be a request per tab.
  }, [appId]);

  useEffect(() => {
    if (wanted) setScreen(wanted);
  }, [wanted]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!app) {
    return <div className="p-6 text-sm text-fg-muted">{error ?? "Loading…"}</div>;
  }

  const manifest = app.declares;
  const view = manifest?.views.find((v) => v.name === screen);
  // Every declared view is reachable, not only those a menu names — a manifest
  // with views and no menu should still be usable rather than blank.
  const tabs =
    manifest?.menu.length
      ? manifest.menu
      : (manifest?.views ?? []).map((v) => ({ label: v.name, view: v.name }));

  const saveManifest = async () => {
    if (editing === null) return;
    setBusy(true);
    setError(null);
    try {
      await api.setAppManifest(app.id, editing);
      setEditing(null);
      refresh();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const uninstall = async () => {
    if (
      !window.confirm(
        `Uninstall ${app.name}? This deletes its tables and everything in them. ` +
          `Switching it off instead keeps the data.`,
      )
    ) {
      return;
    }
    await api.uninstallApp(app.id);
    navigate("/apps");
  };

  return (
    <div className="flex h-full min-h-0 flex-col p-6">
      <div className="mb-4 flex items-center gap-3">
        <span className="text-xl leading-none">{app.icon}</span>
        <div className="min-w-0">
          <h1 className="truncate text-xl font-bold tracking-tight">{app.name}</h1>
          <p className="truncate text-xs text-fg-muted">{app.summary}</p>
        </div>
        <div className="flex-1" />
        {!app.active && (
          <span className="rounded bg-panel-2 px-2 py-1 text-[11px] text-fg-muted">
            Switched off
          </span>
        )}
        {/* Two exports, not one with a checkbox: "here, try my app" and "put
            this on my laptop" are different sentences, and only one of them
            means to include what is in your tables. */}
        <a
          href={api.appExportUrl(app.id, false)}
          download
          title="The app, with empty tables — what you send someone."
          className={buttonClasses({ size: "sm" })}
        >
          Share
        </a>
        <a
          href={api.appExportUrl(app.id, true)}
          download
          title="The app and everything in it — what you carry to another machine."
          className={buttonClasses({ size: "sm" })}
        >
          Export with data
        </a>
        <Link
          to={`/projects/${app.projectId}`}
          title="This app's own folder, in the files editor."
          className={buttonClasses({ size: "sm" })}
        >
          Files
        </Link>
        <Button size="sm" onClick={() => setPerms((p) => !p)}>
          Permissions
        </Button>
        <Button variant="primary" size="sm" onClick={() => setChanging(true)}>
          Change this app
        </Button>
        <Button size="sm" onClick={() => setEditing(editing === null ? app.manifest : null)}>
          {editing === null ? "Manifest" : "Close"}
        </Button>
        <Button variant="danger" size="sm" onClick={uninstall}>
          Uninstall
        </Button>
      </div>

      {app.pending && (
        <SchemaGate appId={app.id} plan={app.pending} onDone={refresh} />
      )}

      {perms && (
        <div className="mb-4">
          <ScopeGrant appId={app.id} />
        </div>
      )}

      {app.manifestError && (
        <div className="mb-4 rounded-xl border border-danger/40 bg-danger-subtle p-4">
          <div className="text-sm font-semibold text-danger-fg">
            This app's manifest has an error, so none of its screens can be drawn.
          </div>
          <pre className="mt-2 whitespace-pre-wrap font-mono text-[11px] text-danger-fg">
            {app.manifestError}
          </pre>
          <Button variant="danger" size="sm" onClick={() => setEditing(app.manifest)} className="mt-3">
            Fix it
          </Button>
        </div>
      )}

      {error && (
        <div className="mb-4 rounded-xl bg-danger-subtle px-3.5 py-2.5 text-xs text-danger-fg">{error}</div>
      )}

      {editing !== null ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <Textarea
            value={editing}
            onChange={(e) => setEditing(e.target.value)}
            spellCheck={false}
            className="min-h-0! flex-1 resize-none rounded-xl! p-3! font-mono text-xs!"
          />
          <div className="mt-3 flex items-center gap-2">
            <Button variant="primary" size="sm" onClick={saveManifest} disabled={busy}>
              Save
            </Button>
            <span className="text-xs text-fg-muted">
              New tables and columns apply themselves. Anything that would lose data waits for
              you.
            </span>
          </div>
        </div>
      ) : (
        <>
          {tabs.length > 1 && (
            <div className="mb-4 flex gap-1 rounded-xl bg-panel-2 p-1">
              {tabs.map((t) => (
                <button
                  key={t.view}
                  onClick={() => setScreen(t.view)}
                  className={`ring-focus relative rounded-lg px-3 py-1.5 text-xs transition-colors ${
                    screen === t.view ? "text-fg" : "text-fg-muted hover:text-fg"
                  }`}
                >
                  {screen === t.view && (
                    <motion.span
                      layoutId="app-tab"
                      transition={springy}
                      className="absolute inset-0 rounded-lg bg-panel shadow-sm"
                    />
                  )}
                  <span className={`relative ${screen === t.view ? "font-semibold" : ""}`}>
                    {t.label}
                  </span>
                </button>
              ))}
            </div>
          )}
          {app.runtime !== "module" ? (
            <>
              <DockerfileGate appId={app.id} onApproved={refresh} />
              {/* With one screen there is no tab bar, and the page's own title
                  is hidden while framed — so the screen would go unnamed. Said
                  here rather than un-hiding the app's h1, which is what makes
                  the frame stop looking like a page in a box, and matching
                  what AppView does for a module in the same position. */}
              {tabs.length === 1 && tabs[0]?.label && (
                <h2 className="mb-2 text-sm font-semibold">{tabs[0].label}</h2>
              )}
              {/* The tab decides the screen; the frame only navigates to it.
                  No tab selected means the app's own front door. */}
              <AppFrame app={app} path={screen ? screenPath(app.runtime, screen) : ""} />
            </>
          ) : manifest && view ? (
            <AppView
              app={app}
              manifest={manifest}
              view={view}
              // Only when there is no tab bar saying it already.
              title={tabs.length > 1 ? undefined : tabs[0]?.label}
              onGoto={setScreen}
            />
          ) : (
            !app.manifestError && (
              <div className="rounded-xl border border-dashed border-border p-8 text-center text-sm text-fg-muted">
                This app declares no views yet. Its tables exist — add a view to the manifest to
                see them.
              </div>
            )
          )}
          <BuildHistory appId={app.id} projectId={app.projectId} onChanged={refresh} />
        </>
      )}

      {changing && (
        <ChangeAppModal
          app={app}
          onClose={() => setChanging(false)}
          onStarted={() => {
            setChanging(false);
            refresh();
          }}
        />
      )}
    </div>
  );
}
