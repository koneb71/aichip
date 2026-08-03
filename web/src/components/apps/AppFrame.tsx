import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, type App, type ContainerState } from "../../lib/api";
import { appOrigin, containerLine } from "../../lib/apps";

/**
 * A container app, embedded.
 *
 * The isolation here is the **distinct origin**, not the `sandbox` attribute.
 * `allow-same-origin` is required rather than conceded: without it the frame
 * gets an opaque origin, which costs it cookies, storage, and its own
 * `/__aichip/*` calls — those fail the bridge's origin check, because
 * `Origin: null` is the absence of an origin rather than this app's.
 *
 * The usual warning that `allow-scripts allow-same-origin` lets a frame remove
 * its own sandbox applies only when the frame is same-origin *with the parent*.
 * It is not: the dashboard is on `localhost` and the app is on
 * `<slug>.app.localhost`. Worth saying, because the next person to read this
 * attribute will flinch at it.
 *
 * No `allow-popups` and no `allow-top-navigation`: an app that can open a
 * window or move the tab has a way out of the CSP that pins it to its own
 * origin.
 */
export const SANDBOX = "allow-scripts allow-same-origin allow-forms allow-modals";

export function AppFrame({ app }: { app: App }) {
  const [state, setState] = useState<ContainerState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolves, setResolves] = useState<boolean | null>(null);

  const refresh = useCallback(
    () => api.appContainer(app.id).then(setState).catch(() => {}),
    [app.id],
  );

  useEffect(() => {
    refresh();
  }, [refresh]);

  // Does this browser resolve *.localhost at all? Asked once, of a name aichip
  // answers to itself, so "your browser cannot get here" is distinguishable
  // from "that container is down" — which otherwise look like the same blank
  // box. Safari has historically sent these to DNS instead of loopback.
  useEffect(() => {
    const port = window.location.port ? `:${window.location.port}` : "";
    fetch(`http://probe.app.localhost${port}/__aichip/health`, { mode: "no-cors" })
      .then(() => setResolves(true))
      .catch(() => setResolves(false));
  }, []);

  const status = state?.preview?.status ?? null;
  const building = status === "building";

  // Only while something is happening. A running container has nothing further
  // to say to this page, and never talks to aichip on its own.
  useEffect(() => {
    if (!building) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [building, refresh]);

  const act = async (start: boolean) => {
    setBusy(true);
    setError(null);
    try {
      setState(start ? await api.startAppContainer(app.id) : null);
      if (!start) await api.stopAppContainer(app.id);
      refresh();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const url = status === "running" ? appOrigin(app.slug, window.location.port) : null;

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Named, always, and outside the frame. An app drawing a convincing
          "grant this permission" dialog inside itself is only a problem if
          there is no line saying where aichip stops and it begins. */}
      <div className="mb-2 flex items-center gap-2 text-xs">
        <span className="text-ink-dim">{containerLine(state)}</span>
        {url && (
          <a href={url} target="_blank" rel="noreferrer" className="font-mono text-accent hover:underline">
            {url.replace(/^https?:\/\//, "")}
          </a>
        )}
        <div className="flex-1" />
        {status !== "running" && (
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => act(true)}
            disabled={busy || building || state?.docker.usable === false}
            className="rounded-lg border border-line px-2 py-1 hover:bg-line/40 disabled:opacity-50"
          >
            {state?.preview?.canWake ? "Wake it" : "Build & run"}
          </motion.button>
        )}
        {status === "running" && (
          <>
            <button
              onClick={refresh}
              className="rounded-lg border border-line px-2 py-1 hover:bg-line/40"
            >
              Reload
            </button>
            <button
              onClick={() => act(false)}
              disabled={busy}
              className="rounded-lg px-2 py-1 text-ink-dim hover:text-ink"
            >
              Stop
            </button>
          </>
        )}
      </div>

      {state?.docker.usable === false && (
        <div className="mb-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900">
          {state.docker.problem} Container apps need it; modules do not.
        </div>
      )}

      {resolves === false && (
        <div className="mb-2 rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-900">
          This browser does not resolve <span className="font-mono">*.localhost</span> to your
          own machine, so a container app cannot be reached from it. Chrome and Firefox do.
          There is no fallback on purpose: serving every app from one address would give them
          all one origin, and the whole permission model is keyed to keeping them apart.
        </div>
      )}

      {error && (
        <div className="mb-2 rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">{error}</div>
      )}
      {state?.preview?.error && (
        <pre className="mb-2 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-red-50 px-3 py-2 font-mono text-[11px] text-danger">
          {state.preview.error}
        </pre>
      )}

      {url ? (
        <iframe
          // Keyed on the URL so Reload actually reloads: React would otherwise
          // keep the same element and the same document.
          key={url}
          src={url}
          title={app.name}
          sandbox={SANDBOX}
          className="min-h-0 flex-1 rounded-xl border border-line bg-white"
        />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center rounded-xl border border-dashed border-line text-sm text-ink-dim">
          {building ? "Building…" : "Not running."}
        </div>
      )}
    </div>
  );
}
