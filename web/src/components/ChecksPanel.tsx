import { useCallback, useEffect, useState } from "react";
import { api, CheckRun } from "../lib/api";
import { checksActive, resultPassed, resultVerdict } from "../lib/checks";
import { duration } from "../lib/runHistory";
import { RunError } from "./ui/RunError";

/**
 * This project's own checks, on this card's worktree.
 *
 * Results first — a person deciding whether to land a diff wants to know if it
 * passes before reading it — then the two actions: run them, or send the
 * failures back to the agent as a follow-up in the same worktree.
 */
export function ChecksPanel({
  taskId,
  busy,
  refreshKey,
  onChanged,
}: {
  taskId: string;
  /** An agent is working on the card: no checks, no fix — the tree is moving. */
  busy: boolean;
  /** Anything that should make the panel look again: the card's newest run and status. */
  refreshKey: string;
  onChanged: () => void;
}) {
  const [configured, setConfigured] = useState(false);
  const [latest, setLatest] = useState<CheckRun | null>(null);
  const [open, setOpen] = useState<string | null>(null);
  const [acting, setActing] = useState<"run" | "fix" | null>(null);
  const [error, setError] = useState<string | null>(null);

  const load = useCallback(() => {
    api
      .taskChecks(taskId)
      .then((r) => {
        setConfigured(r.configured);
        setLatest(r.latest);
      })
      .catch(() => {});
  }, [taskId]);

  useEffect(load, [load, refreshKey]);

  // Poll only while a check run is under way; results land command by command.
  const active = checksActive(latest?.status);
  useEffect(() => {
    if (!active) return;
    const t = setInterval(load, 2000);
    return () => clearInterval(t);
  }, [active, load]);

  if (!configured && !latest) return null;

  const act = async (what: "run" | "fix") => {
    setActing(what);
    setError(null);
    try {
      if (what === "run") await api.runChecks(taskId);
      else await api.fixChecks(taskId);
      load();
      onChanged();
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setActing(null);
    }
  };

  const failing = latest?.status === "failed";

  return (
    <div className="text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <span className="font-semibold uppercase tracking-wide text-[11px] text-ink-dim">Checks</span>
        {latest ? (
          <span className="text-ink-dim">
            {summary(latest)}
            {latest.finishedAt && <> · {latest.startedBy === "auto" ? "ran by themselves" : "you ran these"}</>}
          </span>
        ) : (
          <span className="text-ink-dim">not run on this card yet</span>
        )}
        <div className="ml-auto flex gap-2">
          {failing && (
            <button
              onClick={() => act("fix")}
              disabled={busy || acting !== null}
              title="A follow-up run in this card's worktree, briefed with what failed"
              className="rounded-md bg-accent px-2 py-0.5 text-[11px] font-medium text-white disabled:opacity-40"
            >
              {acting === "fix" ? "Starting…" : "Fix failing checks"}
            </button>
          )}
          {configured && (
            <button
              onClick={() => act("run")}
              disabled={busy || active || acting !== null}
              className="rounded-md border border-line px-2 py-0.5 text-[11px] hover:bg-panel-2 disabled:opacity-40"
            >
              {acting === "run" ? "Starting…" : latest ? "Run again" : "Run checks"}
            </button>
          )}
        </div>
      </div>

      {latest && latest.results.length > 0 && (
        <ul className="mt-2 space-y-1">
          {latest.results.map((r, i) => {
            const key = `${latest.id}:${i}`;
            const ok = resultPassed(r);
            return (
              <li key={key}>
                <button
                  onClick={() => setOpen(open === key ? null : key)}
                  className="flex w-full items-center gap-2 rounded-md px-1 py-0.5 text-left hover:bg-panel-2"
                >
                  <span className={ok ? "text-tier-easy" : "text-danger"}>{ok ? "✓" : "✗"}</span>
                  <span className="font-medium">{r.name}</span>
                  <span className="text-ink-dim">{resultVerdict(r)}</span>
                  <span className="ml-auto tabular-nums text-ink-dim">{duration(Math.round(r.ms / 1000))}</span>
                </button>
                {open === key && (
                  <div className="mt-1">
                    <code className="block px-1 text-[11px] text-ink-dim">$ {r.command}</code>
                    <pre className="mt-1 max-h-64 overflow-auto rounded-md bg-panel-2 p-2 font-mono text-[11px] leading-relaxed">
                      {r.outputTail || "(no output)"}
                    </pre>
                  </div>
                )}
              </li>
            );
          })}
        </ul>
      )}

      {latest && latest.dirtied.length > 0 && (
        <p className="mt-2 rounded-md bg-amber-50 px-2 py-1 text-[11px] text-amber-800">
          The checks left {latest.dirtied.length === 1 ? "a file" : "files"} changed in the worktree —{" "}
          {latest.dirtied.slice(0, 5).join(", ")}
          {latest.dirtied.length > 5 && ` and ${latest.dirtied.length - 5} more`}. Merging would include{" "}
          {latest.dirtied.length === 1 ? "it" : "them"}; add build output to .gitignore.
        </p>
      )}
      {latest?.error && <RunError reason={latest.error} className="mt-2" />}
      {error && <RunError reason={error} className="mt-2" />}
      {configured && !latest && (
        <p className="mt-1 text-[11px] text-ink-dim">
          Checks start by themselves only after a Full Auto run, because they execute code the agent may
          have changed. Run them when you're ready.
        </p>
      )}
    </div>
  );
}

function summary(c: CheckRun): string {
  const passed = c.results.filter(resultPassed).length;
  switch (c.status) {
    case "queued":
      return "waiting for another card's checks to finish…";
    case "running":
      return `running… ${c.results.length} done`;
    case "passed":
      return `all ${passed} passed`;
    case "failed":
      return `${c.results.length - passed} of ${c.results.length} failed`;
    case "canceled":
      return "canceled";
    default:
      return "could not run";
  }
}
