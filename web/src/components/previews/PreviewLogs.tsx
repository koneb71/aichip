import { useCallback, useEffect, useRef, useState } from "react";
import { api } from "../../lib/api";

/**
 * What this preview actually printed.
 *
 * Two halves, because they answer different questions and only one of them
 * exists at a time when things go wrong. The build log explains an image that
 * never came out. The runtime log explains a container that built perfectly and
 * then refused to serve — the case where the card's one-line tail is worse than
 * useless, because the build succeeded and the error is somewhere else entirely.
 *
 * Follows while a build is running, so a multi-minute build is something you
 * can watch rather than wait out.
 */
export function PreviewLogs({
  previewId,
  live,
  onClose,
}: {
  previewId: string;
  /** Still building, so keep reading. */
  live: boolean;
  onClose: () => void;
}) {
  const [build, setBuild] = useState("");
  const [runtime, setRuntime] = useState("");
  const [tab, setTab] = useState<"build" | "runtime">("build");
  const [failed, setFailed] = useState(false);
  const box = useRef<HTMLPreElement>(null);
  const pinned = useRef(true);

  const load = useCallback(
    () =>
      api
        .previewLogs(previewId)
        .then((r) => {
          setBuild(r.build);
          setRuntime(r.runtime);
          // Runtime output only exists once something started; landing on an
          // empty tab looks like the logs are missing.
          if (!r.build.trim() && r.runtime.trim()) setTab("runtime");
        })
        .catch(() => setFailed(true)),
    [previewId],
  );

  useEffect(() => {
    load();
  }, [load]);

  useEffect(() => {
    if (!live) return;
    const t = setInterval(load, 2000);
    return () => clearInterval(t);
  }, [live, load]);

  // Follow the tail, but stop the moment the reader scrolls up — nothing is
  // more annoying than a log that yanks you back while you are reading it.
  const text = tab === "build" ? build : runtime;
  useEffect(() => {
    if (pinned.current && box.current) {
      box.current.scrollTop = box.current.scrollHeight;
    }
  }, [text]);

  return (
    <div className="mt-2 rounded-xl border border-line bg-panel">
      <div className="flex items-center gap-1 border-b border-line px-2 py-1.5">
        {(["build", "runtime"] as const).map((t) => (
          <button
            key={t}
            onClick={() => setTab(t)}
            className={`rounded-md px-2 py-0.5 text-[11px] ${
              tab === t ? "bg-line/60 font-medium" : "text-ink-dim hover:text-ink"
            }`}
          >
            {t === "build" ? "Build" : "Output"}
          </button>
        ))}
        {live && (
          <span className="ml-1 text-[11px] text-ink-dim">following…</span>
        )}
        <button
          onClick={onClose}
          className="ml-auto px-1 text-[11px] text-ink-dim hover:text-ink"
        >
          hide
        </button>
      </div>
      <pre
        ref={box}
        onScroll={(e) => {
          const el = e.currentTarget;
          pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
        }}
        className="max-h-72 overflow-auto whitespace-pre-wrap px-3 py-2 font-mono text-[11px] leading-relaxed"
      >
        {failed
          ? "These logs are no longer available."
          : text.trim() ||
            (tab === "build"
              ? "Nothing yet — the build hasn't started printing."
              : "Nothing yet. A container that never started has no output.")}
      </pre>
    </div>
  );
}
