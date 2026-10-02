import { useState } from "react";
import { Agent, api } from "../../lib/api";
import { RunError } from "../ui/RunError";

/**
 * Whether this agent takes work, and the controls that change it.
 *
 * Pausing is the everyday one: the agent keeps its cards, can still be handed
 * new ones, and starts nothing until resumed — every way a run can start
 * refuses it, with the reason given here. Retiring is the end of the line:
 * gone from the pickers, its history kept.
 */
export function AgentAvailability({ agent, onChanged }: { agent: Agent; onChanged: () => void }) {
  const [reason, setReason] = useState("");
  const [stopNow, setStopNow] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const act = async (fn: () => Promise<unknown>) => {
    setBusy(true);
    setError(null);
    try {
      await fn();
      onChanged();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  if (agent.status === "retired") {
    return (
      <div className="rounded-lg bg-panel-2 px-3 py-2 text-xs text-fg-muted">
        Retired — takes no new work and is hidden from pickers. Its runs and comments still name it.
      </div>
    );
  }

  if (agent.status === "paused") {
    return (
      <div className="rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-800">
        <div className="flex flex-wrap items-center gap-2">
          <span>
            Paused{agent.pauseReason ? ` — ${agent.pauseReason}` : ""}. It keeps its cards and starts
            nothing until resumed.
          </span>
          <button
            onClick={() => act(() => api.resumeAgent(agent.id))}
            disabled={busy}
            className="ml-auto rounded-md bg-accent px-2.5 py-1 text-[11px] font-medium text-white disabled:opacity-40"
          >
            Resume
          </button>
        </div>
        {error && <RunError reason={error} className="mt-2" />}
      </div>
    );
  }

  return (
    <div className="rounded-lg border border-border px-3 py-2.5 text-xs">
      <div className="flex flex-wrap items-center gap-2">
        <input
          value={reason}
          onChange={(e) => setReason(e.target.value)}
          placeholder="Why pause? (optional)"
          className="min-w-0 flex-1 rounded-md border border-border bg-panel px-2 py-1"
        />
        <button
          onClick={() => act(() => api.pauseAgent(agent.id, { reason, stop_now: stopNow }))}
          disabled={busy}
          className="rounded-md border border-border px-2.5 py-1 text-[11px] hover:bg-panel-2 disabled:opacity-40"
        >
          Pause
        </button>
        <button
          onClick={() => act(() => api.retireAgent(agent.id))}
          disabled={busy}
          title="No new work, gone from pickers; its history is kept"
          className="rounded-md px-2 py-1 text-[11px] text-fg-muted hover:text-danger disabled:opacity-40"
        >
          Retire
        </button>
      </div>
      <label className="mt-1.5 flex cursor-pointer items-center gap-2 text-[11px] text-fg-muted">
        <input
          type="checkbox"
          checked={stopNow}
          onChange={(e) => setStopNow(e.target.checked)}
          className="accent-accent"
        />
        Also stop what it is doing now
      </label>
      {error && <RunError reason={error} className="mt-2" />}
    </div>
  );
}
