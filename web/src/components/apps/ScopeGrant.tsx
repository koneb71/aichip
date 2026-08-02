import { useCallback, useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, type AppGrants } from "../../lib/api";
import { ungrantedScopes } from "../../lib/apps";

/**
 * What this app may do with *your* data.
 *
 * Its own tables are absent from this screen on purpose — they exist because
 * it declared them, hold only what it put there, and go when it does. Asking
 * permission to use the thing you installed would be ceremony, and ceremony is
 * what teaches people to click through the questions that matter.
 *
 * What the manifest asks for is shown separately from what it holds, because a
 * rebuild that starts asking for more has to read as a question.
 */
export function ScopeGrant({ appId, onChanged }: { appId: string; onChanged?: () => void }) {
  const [state, setState] = useState<AppGrants | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    api
      .appGrants(appId)
      .then(setState)
      .catch((e) => setError(String(e).replace(/^Error:\s*/, "")));
  }, [appId]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  if (!state) {
    return <div className="text-xs text-ink-dim">{error ?? "Loading…"}</div>;
  }

  const held = state.granted.map((g) => g.scope);
  const asking = ungrantedScopes(state.requested, held);

  const toggle = async (scope: string) => {
    const next = held.includes(scope) ? held.filter((s) => s !== scope) : [...held, scope];
    setBusy(true);
    setError(null);
    try {
      await api.setAppGrants(appId, next);
      refresh();
      onChanged?.();
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="rounded-xl border border-line p-4">
      <h3 className="text-sm font-semibold">Permissions</h3>
      <p className="mt-1 text-xs text-ink-dim">
        This app's own tables need no permission. These are about your data.
      </p>

      {asking.length > 0 && (
        <div className="mt-3 rounded-lg border border-amber-300 bg-amber-50 px-3 py-2 text-xs text-amber-900">
          This app is asking for{" "}
          {asking.map((s) => (
            <span key={s} className="font-mono">
              {s}{" "}
            </span>
          ))}
          — it works without them, minus whatever needed them.
        </div>
      )}

      <div className="mt-3 flex flex-col gap-2">
        {state.all.map((s) => {
          const on = held.includes(s.scope);
          const wanted = state.requested.includes(s.scope);
          const used = state.granted.find((g) => g.scope === s.scope)?.lastUsedAt;
          return (
            <label
              key={s.scope}
              className={
                "flex cursor-pointer items-start gap-3 rounded-lg border p-2 " +
                (wanted ? "border-line" : "border-transparent opacity-60")
              }
            >
              <input
                type="checkbox"
                checked={on}
                disabled={busy}
                onChange={() => toggle(s.scope)}
                className="mt-0.5 h-4 w-4 accent-[var(--color-accent)]"
              />
              <span className="min-w-0 flex-1">
                <span className="flex items-center gap-2">
                  <span className="font-mono text-xs">{s.scope}</span>
                  {s.write && (
                    <span className="rounded bg-amber-100 px-1 text-[10px] text-amber-900">
                      changes things
                    </span>
                  )}
                </span>
                <span className="mt-0.5 block text-xs text-ink-dim">{s.blurb}</span>
                {/* "Granted in August, never used" is the sentence that makes
                    this screen worth opening. */}
                {on && (
                  <span className="mt-0.5 block text-[11px] text-ink-dim">
                    {used ? `Last used ${new Date(used).toLocaleDateString()}` : "Never used"}
                  </span>
                )}
              </span>
            </label>
          );
        })}
      </div>

      {error && <div className="mt-3 text-xs text-danger">{error}</div>}

      {busy && (
        <motion.div initial={{ opacity: 0 }} animate={{ opacity: 1 }} className="mt-2 text-xs text-ink-dim">
          Saving…
        </motion.div>
      )}
    </div>
  );
}
