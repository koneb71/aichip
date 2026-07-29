import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, ModelSettings, Tier } from "../lib/api";

/**
 * Machine-wide settings. Today: which model each complexity tier runs.
 *
 * Tiers are how everything else in the app expresses "how hard is this" — a
 * task's tier, an agent's tier, a workflow step's tier. Which model answers
 * that depends on your plan and your appetite for spend, so it belongs to
 * you rather than to the code.
 */
const TIERS: { key: Tier; label: string; when: string }[] = [
  { key: "easy", label: "Easy", when: "Mechanical, well-specified work." },
  { key: "medium", label: "Medium", when: "Typical feature and bugfix work." },
  { key: "complex", label: "Complex", when: "Architecture, judging, gnarly debugging." },
];

export default function SettingsPage() {
  const [settings, setSettings] = useState<ModelSettings | null>(null);
  const [draft, setDraft] = useState<Record<Tier, string> | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    api
      .modelSettings()
      .then((s) => {
        setSettings(s);
        setDraft(s.tiers);
      })
      .catch((e) => setError(String(e)));
  }, []);

  const dirty =
    !!draft && !!settings && TIERS.some((t) => draft[t.key] !== settings.tiers[t.key]);

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      await api.setModelSettings(draft);
      const fresh = await api.modelSettings();
      setSettings(fresh);
      setDraft(fresh.tiers);
      setSaved(true);
      // Labels elsewhere in the app come from a provider loaded at startup,
      // so a reload is the honest way to make every chip agree at once.
      setTimeout(() => window.location.reload(), 700);
    } catch (e) {
      setError(String(e).replace(/^Error:\s*/, ""));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="h-full overflow-y-auto p-8">
      <h1 className="text-2xl font-bold tracking-tight">Settings</h1>
      <p className="mt-1 max-w-xl text-sm text-ink-dim">
        Which model runs at each complexity tier. Tasks, agents and workflow steps
        all pick a tier — this is what those tiers mean.
      </p>

      <div className="mt-6 max-w-2xl space-y-3">
        {TIERS.map((tier) => (
          <div key={tier.key} className="card-shadow rounded-xl border border-line bg-panel p-4">
            <div className="flex flex-wrap items-center justify-between gap-3">
              <div>
                <div className="text-sm font-semibold">{tier.label}</div>
                <div className="mt-0.5 text-xs text-ink-dim">{tier.when}</div>
              </div>
              <select
                value={draft?.[tier.key] ?? ""}
                onChange={(e) =>
                  setDraft((d) => (d ? { ...d, [tier.key]: e.target.value } : d))
                }
                disabled={!settings}
                className="min-w-52 rounded-lg border border-line bg-panel px-2.5 py-2 text-sm"
              >
                {settings?.choices.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.label}
                  </option>
                ))}
              </select>
            </div>
            {settings && draft && (
              <div className="mt-2 text-[11px] text-ink-dim">
                {settings.choices.find((c) => c.id === draft[tier.key])?.blurb}
                {draft[tier.key] !== settings.defaults[tier.key] && (
                  <span className="ml-1 opacity-70">
                    (default:{" "}
                    {
                      settings.choices.find((c) => c.id === settings.defaults[tier.key])
                        ?.label
                    }
                    )
                  </span>
                )}
              </div>
            )}
          </div>
        ))}
      </div>

      {error && (
        <div className="mt-3 max-w-2xl rounded-lg bg-red-50 px-3 py-2 text-xs text-danger">
          {error}
        </div>
      )}

      <div className="mt-4 flex max-w-2xl items-center gap-3">
        <motion.button
          whileTap={{ scale: 0.96 }}
          onClick={save}
          disabled={!dirty || busy}
          className="rounded-lg bg-accent px-5 py-2 text-sm font-medium text-white disabled:opacity-50"
        >
          {busy ? "Saving…" : saved && !dirty ? "Saved" : "Save"}
        </motion.button>
        {settings && (
          <button
            onClick={() => setDraft(settings.defaults)}
            className="text-xs text-ink-dim hover:text-ink"
          >
            Reset to defaults
          </button>
        )}
        <span className="text-[11px] text-ink-dim">
          Runs already in flight keep the model they started with.
        </span>
      </div>
    </div>
  );
}
