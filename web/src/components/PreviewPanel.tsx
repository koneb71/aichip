import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, DockerStatus, previewUrl, TaskPreview } from "../lib/api";

/**
 * See the branch, not the diff.
 *
 * A card in review is a set of changes, and "does this look right" is a
 * question about the running thing rather than about the patch. This builds
 * the branch's own Dockerfile and gives you a link.
 *
 * Polls only while something is actually happening. A build takes minutes and
 * the status is the only thing that says how it went, but a stopped preview
 * has nothing left to report and polling it forever is just noise on a page
 * that already refreshes every 2.5 seconds.
 */
export function PreviewPanel({ taskId }: { taskId: string }) {
  const [preview, setPreview] = useState<TaskPreview | null>(null);
  const [docker, setDocker] = useState<DockerStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .taskPreview(taskId)
        .then((r) => setPreview(r.preview))
        .catch(() => {}),
    [taskId],
  );

  useEffect(() => {
    setPreview(null);
    setError(null);
    refresh();
    api.dockerStatus().then(setDocker).catch(() => {});
  }, [taskId, refresh]);

  const building = preview?.status === "building";
  useEffect(() => {
    if (!building) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [building, refresh]);

  const start = async () => {
    setBusy(true);
    setError(null);
    try {
      const r = await api.startPreview(taskId);
      setPreview(r.preview);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    try {
      await api.stopPreview(taskId);
      await refresh();
    } finally {
      setBusy(false);
    }
  };

  // Nothing to offer and nothing to explain: don't take up the room.
  if (docker && !docker.installed && !preview) return null;

  const live = preview?.status === "running";
  const alive = live || building;

  return (
    <div className="border-b border-line px-5 py-3">
      <div className="mb-1.5 flex items-center justify-between gap-2">
        <span className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          Preview
        </span>
        {!alive && (
          <motion.button
            whileTap={{ scale: 0.96 }}
            onClick={start}
            disabled={busy || (!!docker && !docker.usable)}
            className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-50"
          >
            {preview?.status === "failed"
              ? "Try again"
              : preview?.canWake
                ? "Wake it"
                : "Build & run"}
          </motion.button>
        )}
        {alive && (
          <div className="flex items-center gap-1.5">
            {live && preview.stale && (
              <button
                onClick={async () => {
                  // Stop then start: one preview per card is the rule the
                  // database enforces, so a rebuild is genuinely both.
                  await stop();
                  await start();
                }}
                disabled={busy}
                className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-50"
              >
                Rebuild
              </button>
            )}
            <button
              onClick={stop}
              disabled={busy}
              className="rounded-lg border border-line px-2.5 py-1 text-xs hover:bg-line/40 disabled:opacity-50"
            >
              Stop
            </button>
          </div>
        )}
      </div>

      {docker && !docker.usable && (
        <div className="rounded-lg bg-amber-50 px-2.5 py-1.5 text-[11px] text-amber-900">
          {docker.problem}
        </div>
      )}

      {building && (
        <div className="flex items-center gap-2 text-xs text-ink-dim">
          <motion.span
            animate={{ opacity: [1, 0.3, 1] }}
            transition={{ repeat: Infinity, duration: 1.4 }}
          >
            ●
          </motion.span>
          Building the image — the first one takes a few minutes.
        </div>
      )}

      {live && previewUrl(preview) && (
        <div className="flex flex-wrap items-baseline gap-2">
          <a
            href={previewUrl(preview)!}
            target="_blank"
            rel="noreferrer"
            className="text-sm font-medium text-accent hover:underline"
          >
            {previewUrl(preview)!.replace(/^https?:\/\//, "")}
          </a>
          {/* Said rather than acted on. Killing what someone is looking at
              because an agent started working is worse than telling them the
              page is from before that — and "what did it look like before?" is
              a question worth being able to answer. */}
          {preview.stale && (
            <span className="rounded-md bg-amber-50 px-1.5 py-0.5 text-[11px] text-amber-900">
              built before the latest run — rebuild to see current work
            </span>
          )}
          {/* Said out loud, because the symptom of a wrong guess is a blank
              page — which reads as a broken branch rather than a wrong port. */}
          {preview.portAssumed && (
            <span className="text-[11px] text-ink-dim">
              its Dockerfile names no port, so port {preview.containerPort} is a
              guess
            </span>
          )}
        </div>
      )}

      {preview?.status === "failed" && preview.error && (
        <pre className="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-lg bg-red-50 px-2.5 py-1.5 text-[11px] leading-relaxed text-danger">
          {preview.error}
        </pre>
      )}

      {error && (
        <div className="mt-1 rounded-lg bg-red-50 px-2.5 py-1.5 text-[11px] text-danger">
          {error}
        </div>
      )}

      {!alive && !error && preview?.status !== "failed" && (
        <div className="text-[11px] text-ink-dim">
          {preview?.canWake
            ? // Worth distinguishing: the button costs seconds here and minutes
              // otherwise, and people plan around which one it is.
              "Stopped because nobody was looking at it. Its image is still here, so waking it takes a few seconds."
            : "Builds this card's branch from its own Dockerfile and serves it at a name of its own. Runs on this machine only."}
        </div>
      )}
    </div>
  );
}
