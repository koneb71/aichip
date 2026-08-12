import { useEffect, useState } from "react";
import { api, AttentionSettingsValue, AttentionEvent } from "../lib/api";

/**
 * How long aichip waits for you, and how it reaches you while it waits.
 *
 * One panel, because from where you sit they are one question. Everything else
 * aichip has is a browser notification from an open tab, which only helps if
 * you are at the machine with the dashboard still up — and a run that takes
 * forty minutes and asks one question at minute three is exactly the case
 * where you are not.
 */
const EVENTS: { id: AttentionEvent; label: string; hint: string }[] = [
  { id: "permission", label: "A run needs permission", hint: "the one that stops work until you answer" },
  { id: "plan", label: "A plan needs review", hint: "a plan-first card, waiting on you" },
  { id: "rate_limited", label: "Rate limited", hint: "it will resume on its own; this just tells you" },
  { id: "over_budget", label: "Daily budget reached", hint: "the queue holds until midnight" },
  { id: "routine", label: "A routine delivered", hint: "it ran on its schedule; the result is waiting" },
  { id: "finished", label: "A run finished", hint: "off by default — it fires on every card" },
];

/** Ready-made commands, so the first one is a paste rather than a project. */
const EXAMPLES: { os: string; command: string }[] = [
  { os: "Linux", command: 'notify-send "$AICHIP_TITLE" "$AICHIP_BODY"' },
  { os: "macOS", command: `osascript -e "display notification \\"$AICHIP_BODY\\" with title \\"$AICHIP_TITLE\\""` },
  { os: "Windows", command: 'powershell -c "[console]::beep(800,400)"' },
  { os: "Phone", command: 'curl -s -d "$AICHIP_BODY" -H "Title: $AICHIP_TITLE" ntfy.sh/your-topic' },
];

export function AttentionSettings() {
  const [v, setV] = useState<AttentionSettingsValue | null>(null);
  const [available, setAvailable] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warning, setWarning] = useState<string | null>(null);

  useEffect(() => {
    api
      .attentionSettings()
      .then(setV)
      // An older server has no such route; the panel removes itself rather
      // than sitting there broken. Same guard PreviewSettings uses.
      .catch(() => setAvailable(false));
  }, []);

  if (!available || !v) return null;

  const save = async (patch: Partial<AttentionSettingsValue>) => {
    setBusy(true);
    setError(null);
    try {
      const saved = await api.setAttentionSettings({ ...v, ...patch });
      setV(saved);
      setWarning(saved.warning ?? null);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  const toggle = (id: AttentionEvent) =>
    save({
      events: v.events.includes(id) ? v.events.filter((e) => e !== id) : [...v.events, id],
    });

  return (
    <section className="mt-8 max-w-2xl">
      <h2 className="text-sm font-semibold">When a run needs you</h2>
      <p className="mt-1 text-xs leading-relaxed text-ink-dim">
        A run that stops to ask something releases its place in the queue, so the rest of the
        board keeps moving. It waits for you — and unlike before, if the wait runs out it is{" "}
        <span className="font-medium text-ink">stopped rather than told you said no</span>.
      </p>

      <div className="mt-4">
        <label className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
          Wait for me
        </label>
        <div className="mt-1.5 flex flex-wrap gap-1.5">
          {[
            { secs: 3600, label: "1 hour" },
            { secs: 8 * 3600, label: "8 hours" },
            { secs: 24 * 3600, label: "1 day" },
            { secs: 0, label: "Indefinitely" },
          ].map((o) => (
            <button
              key={o.secs}
              disabled={busy}
              onClick={() => save({ waitSecs: o.secs })}
              className="rounded-lg border px-2.5 py-1.5 text-xs disabled:opacity-50"
              style={{
                borderColor: v.waitSecs === o.secs ? "var(--color-accent)" : "var(--color-line)",
                color: v.waitSecs === o.secs ? "var(--color-accent)" : "var(--color-ink-dim)",
              }}
            >
              {o.label}
            </button>
          ))}
        </div>
        <p className="mt-1 text-[11px] text-ink-dim/80">
          {v.waitSecs === 0
            ? "It will hold the card until you answer. A waiting run costs nothing but a worktree — it is not using a queue slot."
            : "After that it stops the run and says nobody answered, which is not the same as you refusing."}
        </p>
      </div>

      <label className="mt-5 flex cursor-pointer items-start gap-2 text-sm">
        <input
          type="checkbox"
          checked={v.enabled}
          disabled={busy}
          onChange={(e) => save({ enabled: e.target.checked })}
          className="mt-0.5 accent-[var(--color-accent)]"
        />
        <span className="min-w-0">
          <span className="block font-medium">Run a command to tell me</span>
          <span className="block text-xs text-ink-dim">
            Anything your shell can do — a desktop notification, a beep, a push to your phone.
            This works with the dashboard closed, which browser notifications do not.
          </span>
        </span>
      </label>

      {v.enabled && (
        <div className="mt-3 rounded-xl border border-line bg-surface p-3">
          <input
            defaultValue={v.command}
            disabled={busy}
            onBlur={(e) => e.target.value !== v.command && save({ command: e.target.value })}
            placeholder={EXAMPLES[0].command}
            className="w-full rounded-lg border border-line bg-panel px-2 py-1.5 font-mono text-xs outline-none focus:border-accent"
          />
          <div className="mt-2 flex flex-wrap gap-1.5">
            {EXAMPLES.map((ex) => (
              <button
                key={ex.os}
                disabled={busy}
                onClick={() => save({ command: ex.command })}
                title={ex.command}
                className="rounded-md border border-line px-2 py-0.5 text-[10px] text-ink-dim hover:border-ink-dim hover:text-ink"
              >
                {ex.os}
              </button>
            ))}
          </div>

          <div className="mt-3 text-[11px] text-ink-dim">
            Your command is run as one argument, so nothing from a card can become part of it.
            The details arrive as environment variables:
          </div>
          <div className="mt-1 flex flex-wrap gap-1">
            {v.envNames.map((n) => (
              <code key={n} className="rounded bg-panel-2 px-1.5 py-0.5 font-mono text-[10px]">
                {n}
              </code>
            ))}
          </div>
          {/* Stated rather than left to be discovered, because the absence is
              deliberate and someone will otherwise go looking for it. */}
          <p className="mt-1.5 text-[11px] leading-relaxed text-ink-dim/80">
            What the tool was going to do is <span className="font-medium">not</span> among them.
            A command can forward anywhere, and a Bash input or a file edit carries your code.
            Open the dashboard to see that before you answer.
          </p>

          <div className="mt-3">
            <div className="text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
              Tell me about
            </div>
            {EVENTS.map((e) => (
              <label key={e.id} className="mt-1.5 flex cursor-pointer items-start gap-2 text-xs">
                <input
                  type="checkbox"
                  checked={v.events.includes(e.id)}
                  disabled={busy}
                  onChange={() => toggle(e.id)}
                  className="mt-0.5 accent-[var(--color-accent)]"
                />
                <span className="min-w-0">
                  <span className="block">{e.label}</span>
                  <span className="block text-[11px] text-ink-dim">{e.hint}</span>
                </span>
              </label>
            ))}
          </div>
        </div>
      )}

      {warning && (
        <div className="mt-3 whitespace-pre-wrap rounded-lg bg-amber-50 px-3 py-2 text-[11px] leading-relaxed text-amber-800">
          {warning}
        </div>
      )}
      {error && (
        <div className="mt-3 rounded-lg bg-red-50 px-3 py-2 text-[11px] text-danger">{error}</div>
      )}
    </section>
  );
}
