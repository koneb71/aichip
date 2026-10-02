import { useCallback, useEffect, useState } from "react";
import { api } from "../../lib/api";
import { Button } from "../ui/Button";

/**
 * The Dockerfile a container app builds from, when it is no longer aichip's.
 *
 * Silent in the common case, which is the point of aichip owning the runtime
 * Dockerfiles: nobody reads the fifth agent-written one carefully, and a gate
 * people click through manufactures consent rather than obtaining it. This
 * appears only when the committed file differs from ours — and then it is not
 * optional, because `RUN` executes arbitrary commands on this machine, with
 * the network, at build time.
 */
export function DockerfileGate({
  appId,
  onApproved,
}: {
  appId: string;
  onApproved?: () => void;
}) {
  const [state, setState] = useState<{ text: string | null; drifted: boolean; sha: string | null } | null>(
    null,
  );
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () => api.appDockerfile(appId).then(setState).catch(() => {}),
    [appId],
  );
  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!state?.drifted || !state.sha) return null;

  const approve = async () => {
    setBusy(true);
    setError(null);
    try {
      await api.approveAppDockerfile(appId, state.sha!);
      await refresh();
      onApproved?.();
    } catch (e) {
      // Chiefly the 409 for "it changed while you were reading it", which is
      // the one failure here that matters: an approval must attach to the text
      // that was actually on screen.
      setError(String(e).replace(/^Error:\s*/, ""));
      refresh();
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="mb-4 rounded-xl border border-warning/40 bg-warning-subtle p-4">
      <div className="text-sm font-semibold text-warning-fg">
        This app's Dockerfile is not the one aichip wrote.
      </div>
      <p className="mt-1 text-xs text-warning-fg/80">
        It is what will be built, and its <span className="font-mono">RUN</span> lines execute
        on this machine, with the network. Read it before it does.
      </p>
      <pre className="mt-3 max-h-72 overflow-auto whitespace-pre-wrap rounded-lg bg-panel p-3 font-mono text-[11px]">
        {state.text}
      </pre>
      {error && <div className="mt-2 text-xs text-danger-fg">{error}</div>}
      <Button variant="primary" size="sm" onClick={approve} disabled={busy} className="mt-3">
        I have read it — build from this
      </Button>
    </div>
  );
}
