import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, DockerStatus, ProjectPreview } from "../../lib/api";
import { size } from "../../lib/bytes";
import { RecipeGate } from "../RecipeGate";
import { PreviewLogs } from "./PreviewLogs";

/**
 * Everything this project has running, in one place.
 *
 * Previews were only reachable from inside a card, which meant the one thing
 * you most need to know — what is running *right now*, and what it is costing —
 * could only be assembled by opening cards one at a time. They compete for the
 * same three slots and the same disk, so they belong on one page.
 *
 * Base branch first, because it is what everything else is compared against.
 */
export function PreviewsPanel({ projectId }: { projectId: string }) {
  const [rows, setRows] = useState<ProjectPreview[]>([]);
  const [live, setLive] = useState(0);
  const [maxLive, setMaxLive] = useState(3);
  const [disk, setDisk] = useState(0);
  const [reclaimable, setReclaimable] = useState(0);
  const [docker, setDocker] = useState<DockerStatus | null>(null);
  const [busy, setBusy] = useState<string | null>(null);
  /** Which row has its logs open. One at a time — they are tall. */
  const [showing, setShowing] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(
    () =>
      api
        .projectPreviews(projectId)
        .then((r) => {
          setRows(r.previews);
          setLive(r.live);
          setMaxLive(r.maxLive);
          setDisk(r.diskBytes);
          setReclaimable(r.reclaimable);
        })
        .catch(() => {}),
    [projectId],
  );

  useEffect(() => {
    refresh();
    api.dockerStatus().then(setDocker).catch(() => {});
  }, [refresh]);

  // Only while something is mid-build. A settled list has nothing left to say,
  // and this page is not where a running container reports to.
  const building = rows.some((r) => r.status === "building");
  useEffect(() => {
    if (!building) return;
    const t = setInterval(refresh, 2500);
    return () => clearInterval(t);
  }, [building, refresh]);

  const act = async (key: string, fn: () => Promise<unknown>) => {
    setBusy(key);
    setError(null);
    try {
      await fn();
      await refresh();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(null);
    }
  };

  const base = rows.find((r) => r.taskId === null);

  if (docker && !docker.usable) {
    return (
      <div className="p-6">
        <div className="max-w-xl rounded-xl border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900">
          {docker.problem}
        </div>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto p-6">
      <div className="mb-4 flex flex-wrap items-baseline justify-between gap-3">
        <div>
          <h2 className="text-base font-semibold">Previews</h2>
          <p className="mt-0.5 max-w-xl text-xs text-ink-dim">
            Each card's branch, built and running so you can look at it rather
            than read its diff. They run on this machine and on loopback only.
          </p>
        </div>
        <span className="text-xs text-ink-dim">
          {live} of {maxLive} running · {size(disk)} of images
          {reclaimable > 0 && (
            <>
              {" · "}
              <button
                onClick={() => act("reclaim", api.reclaimPreviewDisk)}
                disabled={busy === "reclaim"}
                className="text-accent hover:underline disabled:opacity-50"
              >
                reclaim {reclaimable}
              </button>
            </>
          )}
        </span>
      </div>

      {error && (
        <div className="mb-3 max-w-2xl rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
          {error}
        </div>
      )}

      <div className="max-w-3xl space-y-2">
        {!base && (
          <Row
            title="main"
            subtitle="The branch cards merge into — run it to compare a card against it."
            action={
              <motion.button
                whileTap={{ scale: 0.96 }}
                onClick={() => act("base", () => api.startBasePreview(projectId))}
                disabled={busy === "base"}
                className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-50"
              >
                Build &amp; run
              </motion.button>
            }
          />
        )}

        {rows.map((r) => (
          <div key={r.id}>
          <Row
            title={r.title}
            badge={r.taskId === null ? "base branch" : undefined}
            badge2={r.isStack ? "stack" : undefined}
            subtitle={detail(r)}
            action={
              <div className="flex items-center gap-1.5">
                {r.status === "running" && r.slug && (
                  <a
                    href={named(r.slug)}
                    target="_blank"
                    rel="noreferrer"
                    className="rounded-lg bg-accent px-2.5 py-1 text-xs font-medium text-white"
                  >
                    Open
                  </a>
                )}
                {(r.status === "idle" || r.status === "failed") && (
                  <button
                    onClick={() =>
                      act(r.id, () =>
                        r.taskId
                          ? api.startPreview(r.taskId)
                          : api.startBasePreview(projectId),
                      )
                    }
                    disabled={busy === r.id}
                    className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-50"
                  >
                    {r.canWake ? "Wake" : "Try again"}
                  </button>
                )}
                {r.status !== "failed" && (
                  <button
                    onClick={() =>
                      act(r.id, () =>
                        r.taskId
                          ? api.stopPreview(r.taskId)
                          : api.stopBasePreview(projectId),
                      )
                    }
                    disabled={busy === r.id}
                    className="rounded-lg border border-line px-2.5 py-1 text-xs hover:bg-line/40 disabled:opacity-50"
                  >
                    Stop
                  </button>
                )}
                {/* Offered on every row, not only failures: a preview that
                    built fine and serves the wrong thing is the case with no
                    error message at all. */}
                <button
                  onClick={() => setShowing(showing === r.id ? null : r.id)}
                  className="rounded-lg px-1.5 py-1 text-xs text-ink-dim hover:text-ink"
                >
                  {showing === r.id ? "hide logs" : "logs"}
                </button>
              </div>
            }
          />
          {showing === r.id && (
            <PreviewLogs
              previewId={r.id}
              live={r.status === "building"}
              onClose={() => setShowing(null)}
            />
          )}
          </div>
        ))}

        {rows.length === 0 && (
          <p className="text-xs text-ink-dim">
            Nothing running. Open a card and press Build &amp; run, or start the
            base branch above.
          </p>
        )}
      </div>

      <div className="mt-6 max-w-3xl">
        <div className="mb-1.5 text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          How this gets built
        </div>
        <p className="mb-1.5 text-xs text-ink-dim">
          Used when a branch has no Dockerfile and no compose file of its own. An
          agent reads the project and decides which it needs; you read that
          before anything builds it.
        </p>
        <RecipeGate projectId={projectId} onApproved={refresh} />
      </div>
    </div>
  );
}

function Row({
  title,
  badge,
  badge2,
  subtitle,
  action,
}: {
  title: string;
  badge?: string;
  /** e.g. "stack" — several services, not one container. */
  badge2?: string;
  subtitle: React.ReactNode;
  action: React.ReactNode;
}) {
  return (
    <div className="card-shadow flex flex-wrap items-center justify-between gap-3 rounded-xl border border-line bg-panel p-3">
      <div className="min-w-0 flex-1">
        <div className="flex items-baseline gap-2">
          <span className="truncate text-sm font-semibold">{title}</span>
          {[badge, badge2].filter(Boolean).map((b) => (
            <span
              key={b}
              className="rounded-md bg-line/60 px-1.5 py-0.5 text-[10px] uppercase tracking-wide text-ink-dim"
            >
              {b}
            </span>
          ))}
        </div>
        <div className="mt-0.5 text-[11px] text-ink-dim">{subtitle}</div>
      </div>
      {action}
    </div>
  );
}

/** What this row's state actually means, in a line. */
function detail(r: ProjectPreview): React.ReactNode {
  if (r.status === "building")
    return r.isStack
      ? "Building the stack — every service, so this takes a while."
      : "Building — the first one takes a few minutes.";
  if (r.status === "failed")
    return (
      <span className="text-danger">
        {(r.error ?? "Build failed.").split("\n").slice(-1)[0]}
      </span>
    );
  if (r.status === "idle")
    return r.canWake
      ? "Stopped while nobody was looking. Its image is here, so waking takes seconds."
      : "Stopped. Its image is gone, so this rebuilds from scratch.";
  return (
    <span className="flex flex-wrap items-baseline gap-2">
      <span className="font-mono">{r.slug}.preview.localhost</span>
      {r.stale && (
        <span className="rounded bg-amber-50 px-1 text-amber-900">
          built before the latest run
        </span>
      )}
      {r.portAssumed && (
        <span>its Dockerfile names no port, so {r.containerPort} is a guess</span>
      )}
    </span>
  );
}

/** The port is this page's own — aichip proxies preview names on it. */
function named(slug: string): string {
  const port = window.location.port ? `:${window.location.port}` : "";
  return `http://${slug}.preview.localhost${port}`;
}

