import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, DockerStatus, previewUrl, TaskPreview } from "../lib/api";
import { RecipeGate } from "./RecipeGate";

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
export function PreviewPanel({
  taskId,
  projectId,
}: {
  taskId: string;
  projectId: string;
}) {
  const [preview, setPreview] = useState<TaskPreview | null>(null);
  const [docker, setDocker] = useState<DockerStatus | null>(null);
  /**
   * The same project's base branch, running.
   *
   * Offered here rather than on its own page because the question it answers
   * only comes up while you are looking at a card: "is this different from
   * main, or was main always like that?" A diff cannot tell you.
   */
  const [base, setBase] = useState<TaskPreview | null>(null);
  const [baseBusy, setBaseBusy] = useState(false);
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
    api.basePreview(projectId).then((r) => setBase(r.preview)).catch(() => {});
  }, [taskId, projectId, refresh]);

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
  // Matched on the message because it is the message the server sends; the
  // alternative is an error code nobody reads, for one case.
  const noDockerfile = !alive && !!error?.includes("no Dockerfile");

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

      {/* The one failure with a way out: no Dockerfile is a thing an agent can
          fix, and it is the state most projects start in. Shown only for that
          error, so the gate does not appear next to unrelated build failures. */}
      {noDockerfile && (
        <RecipeGate projectId={projectId} onApproved={start} />
      )}

      {live && (
        <div className="mt-1.5 flex flex-wrap items-baseline gap-2 text-[11px]">
          <span className="text-ink-dim">Compare with main:</span>
          {base?.status === "running" && previewUrl(base) ? (
            <>
              <a
                href={previewUrl(base)!}
                target="_blank"
                rel="noreferrer"
                className="text-accent hover:underline"
              >
                {previewUrl(base)!.replace(/^https?:\/\//, "")}
              </a>
              <button
                onClick={async () => {
                  setBaseBusy(true);
                  try {
                    await api.stopBasePreview(projectId);
                    setBase(null);
                  } finally {
                    setBaseBusy(false);
                  }
                }}
                disabled={baseBusy}
                className="text-ink-dim hover:text-ink disabled:opacity-50"
              >
                stop
              </button>
            </>
          ) : (
            <button
              onClick={async () => {
                setBaseBusy(true);
                try {
                  const r = await api.startBasePreview(projectId);
                  setBase(r.preview);
                  // It builds in the background like any other preview.
                  const t = setInterval(async () => {
                    const next = await api.basePreview(projectId);
                    setBase(next.preview);
                    if (next.preview?.status !== "building") clearInterval(t);
                  }, 2500);
                } catch {
                  // The message belongs to the base preview, and there is
                  // nowhere sensible to put a second error block here.
                } finally {
                  setBaseBusy(false);
                }
              }}
              disabled={baseBusy || base?.status === "building"}
              className="text-accent hover:underline disabled:opacity-50"
            >
              {base?.status === "building" ? "building…" : "run it too"}
            </button>
          )}
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
