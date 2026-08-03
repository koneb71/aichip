import { useCallback, useEffect, useState } from "react";
import { AnimatePresence, motion } from "framer-motion";
import { Link, useNavigate, useParams, useSearchParams } from "react-router-dom";
import { api, type AppDetail } from "../lib/api";
import { AppView } from "../components/apps/AppView";
import { SchemaGate } from "../components/apps/SchemaGate";
import { ScopeGrant } from "../components/apps/ScopeGrant";
import { AppFrame } from "../components/apps/AppFrame";
import { BuildHistory } from "../components/apps/BuildHistory";
import { ChangeAppModal } from "../components/apps/ChangeAppModal";
import { DockerfileGate } from "../components/apps/DockerfileGate";

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
        setScreen(
          (s) => s ?? wanted ?? a.declares?.menu[0]?.view ?? a.declares?.views[0]?.name ?? null,
        );
      })
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
  }, [appId, wanted]);

  // A later click on a different sidebar entry is a new `?view=`, and the state
  // above has already been set by then.
  useEffect(() => {
    if (wanted) setScreen(wanted);
  }, [wanted]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!app) {
    return <div className="p-6 text-sm text-ink-dim">{error ?? "Loading…"}</div>;
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
          <h1 className="truncate text-lg font-semibold">{app.name}</h1>
          <p className="truncate text-xs text-ink-dim">{app.summary}</p>
        </div>
        <div className="flex-1" />
        {!app.active && (
          <span className="rounded bg-panel-2 px-2 py-1 text-[11px] text-ink-dim">
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
          className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
        >
          Share
        </a>
        <a
          href={api.appExportUrl(app.id, true)}
          download
          title="The app and everything in it — what you carry to another machine."
          className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
        >
          Export with data
        </a>
        <Link
          to={`/projects/${app.projectId}`}
          title="This app's own folder, in the files editor."
          className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
        >
          Files
        </Link>
        <button
          onClick={() => setPerms((p) => !p)}
          className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
        >
          Permissions
        </button>
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={() => setChanging(true)}
          className="rounded-lg bg-accent px-2 py-1 text-xs font-medium text-white"
        >
          Change this app
        </motion.button>
        <button
          onClick={() => setEditing(editing === null ? app.manifest : null)}
          className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40"
        >
          {editing === null ? "Manifest" : "Close"}
        </button>
        <button
          onClick={uninstall}
          className="rounded-lg px-2 py-1 text-xs text-danger hover:bg-red-50"
        >
          Uninstall
        </button>
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
        <div className="mb-4 rounded-xl border border-red-300 bg-red-50 p-4">
          <div className="text-sm font-semibold text-danger">
            This app's manifest has an error, so none of its screens can be drawn.
          </div>
          <pre className="mt-2 whitespace-pre-wrap font-mono text-[11px] text-danger">
            {app.manifestError}
          </pre>
          <button
            onClick={() => setEditing(app.manifest)}
            className="mt-3 rounded-lg border border-red-300 px-2 py-1 text-xs text-danger"
          >
            Fix it
          </button>
        </div>
      )}

      {error && (
        <div className="mb-4 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}

      {editing !== null ? (
        <div className="flex min-h-0 flex-1 flex-col">
          <textarea
            value={editing}
            onChange={(e) => setEditing(e.target.value)}
            spellCheck={false}
            className="min-h-0 flex-1 resize-none rounded-xl border border-line bg-surface p-3 font-mono text-xs outline-none focus:border-accent"
          />
          <div className="mt-3 flex items-center gap-2">
            <motion.button
              whileTap={{ scale: 0.96 }}
              onClick={saveManifest}
              disabled={busy}
              className="rounded-lg bg-accent px-3 py-1.5 text-xs font-medium text-white disabled:opacity-50"
            >
              Save
            </motion.button>
            <span className="text-xs text-ink-dim">
              New tables and columns apply themselves. Anything that would lose data waits for
              you.
            </span>
          </div>
        </div>
      ) : (
        <>
          {tabs.length > 1 && (
            <div className="mb-4 flex gap-1 border-b border-line">
              {tabs.map((t) => (
                <button
                  key={t.view}
                  onClick={() => setScreen(t.view)}
                  className={
                    "-mb-px border-b-2 px-3 py-1.5 text-xs " +
                    (screen === t.view
                      ? "border-accent font-medium text-ink"
                      : "border-transparent text-ink-dim hover:text-ink")
                  }
                >
                  {t.label}
                </button>
              ))}
            </div>
          )}
          {app.runtime !== "module" ? (
            <>
              <DockerfileGate appId={app.id} onApproved={refresh} />
              <AppFrame app={app} />
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
              <div className="rounded-xl border border-dashed border-line p-8 text-center text-sm text-ink-dim">
                This app declares no views yet. Its tables exist — add a view to the manifest to
                see them.
              </div>
            )
          )}
          <BuildHistory appId={app.id} projectId={app.projectId} onChanged={refresh} />
        </>
      )}

      <AnimatePresence>
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
      </AnimatePresence>
    </div>
  );
}
