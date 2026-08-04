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

/**
 * `path` is which screen the frame shows — `/tasks` for a node app's
 * `views/tasks.html`, `/tasks.html` for static. The tab bar owns the choice;
 * this component only navigates. Empty means the app's front door.
 */
export function AppFrame({ app, path = "" }: { app: App; path?: string }) {
  const [state, setState] = useState<ContainerState | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resolves, setResolves] = useState<boolean | null>(null);
  const [menu, setMenu] = useState(false);

  // A menu with no way out is a trap: the only affordance that dismisses it
  // would otherwise be picking one of its items. Escape and a click anywhere
  // else both close it, and the listeners exist only while it is open.
  useEffect(() => {
    if (!menu) return;
    const close = () => setMenu(false);
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") setMenu(false);
    };
    // Capture, so a click on the button itself still toggles before this runs
    // on the next tick rather than fighting it.
    window.addEventListener("click", close);
    window.addEventListener("keydown", key);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", key);
    };
  }, [menu]);

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
  // The menu only exists while something is running; a container that stopped
  // on its own would otherwise leave it floating over a dead frame.
  if (menu && status !== "running") setMenu(false);

  return (
    <div className="flex min-h-0 flex-1 flex-col">
      {/* Where aichip stops and the app begins, said once and quietly.
          Deliberately not removed when everything is fine: an app can draw a
          convincing "aichip needs your token" dialog inside itself, and the
          only thing that gives it away is a line saying the content below is
          the app's. What moved into the menu is the *controls* — they are
          maintenance, and maintenance does not belong in the middle of a
          screen someone is using. */}
      <div className="mb-2 flex items-center gap-2 text-[11px] text-ink-dim">
        {url ? (
          <>
            <span className="size-1.5 rounded-full bg-emerald-500" />
            <span>
              running in this app&rsquo;s own container at{" "}
              <a
                href={url}
                target="_blank"
                rel="noreferrer"
                className="font-mono hover:text-ink hover:underline"
              >
                {url.replace(/^https?:\/\//, "")}
              </a>
            </span>
          </>
        ) : (
          <span>{containerLine(state)}</span>
        )}
        <div className="flex-1" />
        {status !== "running" && (
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={() => act(true)}
            disabled={busy || building || state?.docker.usable === false}
            className="rounded-lg border border-line px-2 py-1 text-xs hover:bg-line/40 disabled:opacity-50"
          >
            {state?.preview?.canWake ? "Wake it" : "Build & run"}
          </motion.button>
        )}
        {status === "running" && (
          <div className="relative">
            <button
              onClick={(e) => {
                e.stopPropagation();
                setMenu((m) => !m);
              }}
              aria-haspopup="menu"
              aria-expanded={menu}
              aria-label="Container controls"
              className="rounded px-1.5 py-0.5 hover:bg-line/40 hover:text-ink"
            >
              &#8943;
            </button>
            {menu && (
              <div className="card-shadow absolute right-0 z-10 mt-1 w-36 rounded-lg border border-line bg-panel py-1 text-xs">
                <button
                  onClick={() => {
                    setMenu(false);
                    refresh();
                  }}
                  className="block w-full px-3 py-1.5 text-left hover:bg-line/40"
                >
                  Reload
                </button>
                <button
                  onClick={() => {
                    setMenu(false);
                    act(false);
                  }}
                  disabled={busy}
                  className="block w-full px-3 py-1.5 text-left hover:bg-line/40 disabled:opacity-50"
                >
                  Stop container
                </button>
              </div>
            )}
          </div>
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
          // Keyed on the full address so Reload reloads and switching tabs
          // actually navigates: React would otherwise keep the same element
          // and the same document.
          key={url + path}
          src={url + path}
          title={app.name}
          sandbox={SANDBOX}
          // No border, no card, no white: the app's own stylesheet leaves its
          // background transparent when framed, so what shows through is the
          // dashboard's surface and the seam disappears.
          className="min-h-0 flex-1 bg-transparent"
        />
      ) : (
        <div className="flex min-h-0 flex-1 items-center justify-center rounded-xl border border-dashed border-line text-sm text-ink-dim">
          {building ? "Building…" : "Not running."}
        </div>
      )}
    </div>
  );
}
