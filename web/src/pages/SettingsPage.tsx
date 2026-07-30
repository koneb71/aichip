import { useEffect, useState } from "react";
import { motion } from "framer-motion";
import { api, EffortSettings, EngineModels, ModelSettings, PermissionMode, PermissionSettings, Tier } from "../lib/api";

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

/** `{engine_id: {easy, medium, complex}}` — what the Save button sends. */
type Draft = Record<string, Record<Tier, string>>;

const asDraft = (s: ModelSettings): Draft =>
  Object.fromEntries(s.engines.map((e) => [e.id, { ...e.tiers }]));

export default function SettingsPage() {
  const [settings, setSettings] = useState<ModelSettings | null>(null);
  const [draft, setDraft] = useState<Draft | null>(null);
  const [saved, setSaved] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [perms, setPerms] = useState<PermissionSettings | null>(null);
  const [effort, setEffort] = useState<EffortSettings | null>(null);

  useEffect(() => {
    api.permissionSettings().then(setPerms).catch(() => {});
    api.effortSettings().then(setEffort).catch(() => {});
  }, []);

  useEffect(() => {
    api
      .modelSettings()
      .then((s) => {
        setSettings(s);
        setDraft(asDraft(s));
      })
      .catch((e) => setError(String(e)));
  }, []);

  const dirty =
    !!draft &&
    !!settings &&
    settings.engines.some((e) => TIERS.some((t) => draft[e.id]?.[t.key] !== e.tiers[t.key]));

  const save = async () => {
    if (!draft) return;
    setBusy(true);
    setError(null);
    try {
      await api.setModelSettings(draft);
      const fresh = await api.modelSettings();
      setSettings(fresh);
      setDraft(asDraft(fresh));
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

      <h2 className="mt-7 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Permissions
      </h2>
      <p className="mt-1 max-w-xl text-sm text-ink-dim">
        How much new work is allowed to do before it stops to ask you.
      </p>
      <div className="mt-3 max-w-2xl space-y-2">
        {perms?.modes.map((m) => (
          <label
            key={m.id}
            className={`flex cursor-pointer items-start gap-2.5 rounded-xl border p-3 ${
              perms.defaultMode === m.id ? "border-accent bg-accent/5" : "border-line bg-panel"
            }`}
          >
            <input
              type="radio"
              name="permission-mode"
              checked={perms.defaultMode === m.id}
              onChange={async () => {
                setPerms({ ...perms, defaultMode: m.id });
                await api.setDefaultPermissionMode(m.id as PermissionMode);
              }}
              className="mt-0.5 accent-[var(--color-accent)]"
            />
            <span className="min-w-0">
              <span className="block text-sm font-medium">{m.label}</span>
              <span className="mt-0.5 block text-xs text-ink-dim">{m.blurb}</span>
            </span>
          </label>
        ))}
        <p className="text-[11px] text-ink-dim">
          Applies to cards created from now on. "Don't ask" also needs the project
          itself to opt in — that switch is on the project, next to its name.
        </p>
        {!!perms?.agentsOverriding && (
          <div className="rounded-xl border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900">
            <span className="font-semibold">
              {perms.agentsOverriding} agent{perms.agentsOverriding === 1 ? "" : "s"} set
              their own permission mode
            </span>{" "}
            — a card's agent overrides the setting above, so those runs will keep
            asking whatever you choose here.
            <button
              onClick={async () => {
                await api.applyPermissionsToAgents();
                setPerms(await api.permissionSettings());
              }}
              className="ml-2 rounded-lg border border-amber-400 bg-panel px-2.5 py-1 font-medium hover:bg-amber-100"
            >
              Make them follow this setting
            </button>
          </div>
        )}
      </div>

      <h2 className="mt-8 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Thinking
      </h2>
      <p className="mt-1 max-w-xl text-sm text-ink-dim">
        How hard the model works before it answers. Separate from which model —
        the same model can think for a second or for several minutes, and the
        second one costs a great deal more.
      </p>
      <div className="mt-3 max-w-2xl space-y-2">
        <EffortChoice
          checked={effort?.defaultEffort == null}
          label="Leave it to the CLI"
          blurb="Whatever claude or opencode does on its own. This is what aichip ships with."
          onPick={async () => {
            setEffort((e) => (e ? { ...e, defaultEffort: null } : e));
            await api.setDefaultEffort(null);
          }}
        />
        {effort?.levels.map((l) => (
          <EffortChoice
            key={l.id}
            checked={effort.defaultEffort === l.id}
            label={l.label}
            blurb={l.blurb}
            onPick={async () => {
              setEffort({ ...effort, defaultEffort: l.id });
              await api.setDefaultEffort(l.id);
            }}
          />
        ))}
        <p className="text-[11px] text-ink-dim">
          Resolved when a run starts, not when a card is made — so raising this
          reaches work already sitting in the backlog. A card or its agent can
          still pin its own.
        </p>
        {!!effort?.agentsOverriding && (
          <div className="rounded-xl border border-amber-300 bg-amber-50 p-3 text-xs text-amber-900">
            <span className="font-semibold">
              {effort.agentsOverriding} agent
              {effort.agentsOverriding === 1 ? "" : "s"} set their own
            </span>{" "}
            — an agent's budget outranks this, the same way its permission preset
            does. Change those on the agent itself.
          </div>
        )}
      </div>

      <h2 className="mt-8 text-sm font-semibold uppercase tracking-wider text-ink-dim">
        Models
      </h2>
      <p className="mt-1 max-w-2xl text-sm text-ink-dim">
        A tier means a different model on each engine — "medium" can't name one
        model globally, since OpenCode has never heard of{" "}
        <code className="text-[11px]">claude-opus-5</code>.
      </p>
      <div className="mt-3 max-w-2xl space-y-5">
        {settings?.engines.map((engine) => (
          <div key={engine.id}>
            <div className="mb-2 flex flex-wrap items-baseline gap-2">
              <span className="text-sm font-semibold">{engine.label}</span>
              {!!engine.providers.length && (
                <span className="text-[11px] text-ink-dim">
                  signed in with {engine.providers.map((p) => p.name).join(", ")}
                </span>
              )}
            </div>
            <div className="space-y-3">
              {TIERS.map((tier) => (
                <div
                  key={tier.key}
                  className="card-shadow rounded-xl border border-line bg-panel p-4"
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div>
                      <div className="text-sm font-semibold">{tier.label}</div>
                      <div className="mt-0.5 text-xs text-ink-dim">{tier.when}</div>
                    </div>
                    <TierField
                      engine={engine}
                      value={draft?.[engine.id]?.[tier.key] ?? ""}
                      onChange={(v) =>
                        setDraft((d) =>
                          d
                            ? { ...d, [engine.id]: { ...d[engine.id], [tier.key]: v } }
                            : d,
                        )
                      }
                    />
                  </div>
                  {draft && (
                    <div className="mt-2 text-[11px] text-ink-dim">
                      {engine.choices.find(
                        (c) => c.id === draft[engine.id]?.[tier.key],
                      )?.blurb ??
                        (engine.fixedCatalog
                          ? null
                          : "Any provider/model id this engine can reach.")}
                      {draft[engine.id]?.[tier.key] !== engine.defaults[tier.key] && (
                        <span className="ml-1 opacity-70">
                          (default:{" "}
                          {engine.choices.find((c) => c.id === engine.defaults[tier.key])
                            ?.label ?? engine.defaults[tier.key]}
                          )
                        </span>
                      )}
                    </div>
                  )}
                </div>
              ))}
            </div>
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
            onClick={() =>
              setDraft(
                Object.fromEntries(
                  settings.engines.map((e) => [e.id, { ...e.defaults }]),
                ),
              )
            }
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

/**
 * A fixed catalog gets a picker; everything else gets free text.
 *
 * OpenCode fronts 75+ providers plus local models, so a dropdown would be
 * both wrong within a week and unable to express `ollama/qwen3-coder`. The
 * server still validates the `provider/model` shape, which catches the one
 * mistake people actually make: pasting a Claude id into this field.
 */
/** One radio in the thinking list. Same shape as a permission mode. */
function EffortChoice({
  checked,
  label,
  blurb,
  onPick,
}: {
  checked: boolean;
  label: string;
  blurb: string;
  onPick: () => void;
}) {
  return (
    <label
      className={`flex cursor-pointer items-start gap-2.5 rounded-xl border p-3 ${
        checked ? "border-accent bg-accent/5" : "border-line bg-panel"
      }`}
    >
      <input
        type="radio"
        name="default-effort"
        checked={checked}
        onChange={onPick}
        className="mt-0.5 accent-[var(--color-accent)]"
      />
      <span className="min-w-0">
        <span className="block text-sm font-medium">{label}</span>
        <span className="mt-0.5 block text-xs text-ink-dim">{blurb}</span>
      </span>
    </label>
  );
}

function TierField({
  engine,
  value,
  onChange,
}: {
  engine: EngineModels;
  value: string;
  onChange: (v: string) => void;
}) {
  if (engine.fixedCatalog) {
    return (
      <select
        value={value}
        onChange={(e) => onChange(e.target.value)}
        className="min-w-52 rounded-lg border border-line bg-panel px-2.5 py-2 text-sm"
      >
        {engine.choices.map((c) => (
          <option key={c.id} value={c.id}>
            {c.label}
          </option>
        ))}
      </select>
    );
  }
  // A datalist rather than a select: the CLI's list is what this machine can
  // reach today, but a local model it doesn't enumerate is still legitimate.
  const listId = `models-${engine.id}`;
  return (
    <div className="flex flex-col items-end gap-1">
      <input
        value={value}
        list={listId}
        onChange={(e) => onChange(e.target.value)}
        spellCheck={false}
        placeholder="provider/model"
        className="min-w-64 rounded-lg border border-line bg-panel px-2.5 py-2 font-mono text-xs"
      />
      <datalist id={listId}>
        {engine.available.map((id) => (
          <option key={id} value={id} />
        ))}
      </datalist>
      {!engine.available.includes(value) && !!engine.available.length && (
        <span className="text-[10px] text-amber-700">
          not in this install's model list
        </span>
      )}
    </div>
  );
}
