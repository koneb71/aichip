import { useCallback, useEffect, useState } from "react";
import { api } from "../lib/api";

/**
 * What previews are allowed to cost.
 *
 * Both numbers exist because the machine running previews is also running the
 * user's editor. The limit stops a column of cards becoming the whole machine;
 * the idle window stops the one you forgot about holding memory until Friday.
 */
export function PreviewSettings() {
  const [maxLive, setMaxLive] = useState(3);
  const [idle, setIdle] = useState(30);
  const [live, setLive] = useState(0);
  const [disk, setDisk] = useState<{ bytes: number; reclaimable: number } | null>(null);
  const [available, setAvailable] = useState(true);
  const [busy, setBusy] = useState(false);

  const refresh = useCallback(async () => {
    try {
      const l = await api.previewLimits();
      setMaxLive(l.maxLive);
      setIdle(l.idleMinutes);
      setLive(l.live);
      setDisk(await api.previewDisk());
    } catch {
      // An older server has none of these routes. The section is about a
      // feature that server does not have, so it is not shown at all.
      setAvailable(false);
    }
  }, []);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!available) return null;

  const save = async (next: { max?: number; idle?: number }) => {
    const m = next.max ?? maxLive;
    const i = next.idle ?? idle;
    setMaxLive(m);
    setIdle(i);
    await api.setPreviewLimits(m, i);
  };

  return (
    <>
      <h2 className="mt-8 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Previews
      </h2>
      <p className="mt-1 max-w-xl text-sm text-ink-dim">
        Running a card's branch so you can look at it. Each one holds memory and
        CPU on this machine — the same machine your editor is on.
      </p>
      <div className="mt-3 max-w-2xl space-y-3">
        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold">How many at once</div>
              <div className="mt-0.5 text-xs text-ink-dim">
                Each preview is capped at 2 GB and 2 CPUs. {live} running now.
              </div>
            </div>
            <input
              type="number"
              min={1}
              max={20}
              value={maxLive}
              onChange={(e) => save({ max: Number(e.target.value) })}
              className="w-20 rounded-lg border border-line bg-panel px-2 py-1 text-sm"
            />
          </div>
        </div>

        <div className="card-shadow rounded-xl border border-line bg-panel p-4">
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold">Stop after idle</div>
              <div className="mt-0.5 text-xs text-ink-dim">
                {/* The kept image is the whole reason this is safe to do
                    automatically, so it is the thing worth saying. */}
                Minutes with nobody looking. The image is kept, so coming back
                takes seconds. Zero never stops one.
              </div>
            </div>
            <input
              type="number"
              min={0}
              max={1440}
              value={idle}
              onChange={(e) => save({ idle: Number(e.target.value) })}
              className="w-20 rounded-lg border border-line bg-panel px-2 py-1 text-sm"
            />
          </div>
        </div>

        {!!disk && (
          <div className="card-shadow rounded-xl border border-line bg-panel p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <div className="text-sm font-semibold">
                  {gb(disk.bytes)} of preview images
                </div>
                <div className="mt-0.5 text-xs text-ink-dim">
                  {disk.reclaimable > 0
                    ? `${disk.reclaimable} belong to previews that aren't running. Reclaiming turns their next start back into a full rebuild.`
                    : "Nothing to reclaim — every image here belongs to a running preview."}
                </div>
              </div>
              <button
                disabled={busy || disk.reclaimable === 0}
                onClick={async () => {
                  setBusy(true);
                  try {
                    await api.reclaimPreviewDisk();
                    await refresh();
                  } finally {
                    setBusy(false);
                  }
                }}
                className="rounded-lg border border-line px-2.5 py-1 text-xs font-medium hover:bg-line/40 disabled:opacity-40"
              >
                Reclaim
              </button>
            </div>
          </div>
        )}
      </div>
    </>
  );
}

/** Docker's own units, so the figure matches what `docker images` prints. */
function gb(bytes: number): string {
  if (bytes >= 1e9) return `${(bytes / 1e9).toFixed(1)} GB`;
  if (bytes >= 1e6) return `${Math.round(bytes / 1e6)} MB`;
  return `${bytes} B`;
}
