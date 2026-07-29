import { createContext, useContext, useEffect, useState } from "react";
import { api, ModelSettings, Tier } from "./api";

/**
 * Which model each tier actually routes to.
 *
 * The tier labels used to be hard-coded — "complex" always read "Fable 5" —
 * which was fine only while the mapping itself was hard-coded. Now that it's
 * a setting, a label baked into the bundle would confidently name a model
 * the run isn't using.
 */
const Ctx = createContext<ModelSettings | null>(null);

export function ModelsProvider({ children }: { children: React.ReactNode }) {
  const [settings, setSettings] = useState<ModelSettings | null>(null);

  useEffect(() => {
    api.modelSettings().then(setSettings).catch(() => {});
  }, []);

  return <Ctx.Provider value={settings}>{children}</Ctx.Provider>;
}

/** Refetch after the settings page saves, so labels update everywhere. */
export function useModelSettings(): ModelSettings | null {
  return useContext(Ctx);
}

/** Fallbacks for the first paint, before the fetch lands. */
const PLACEHOLDER: Record<Tier, string> = {
  easy: "Sonnet",
  medium: "Opus",
  complex: "Opus",
};

/**
 * A tier's model as a short label, e.g. `medium` → "Opus 5".
 *
 * Falls back to the model id when the user picked something the catalog
 * doesn't name — better a raw id than a blank chip.
 */
export function useTierModel(): (tier: Tier) => string {
  const settings = useContext(Ctx);
  return (tier: Tier) => {
    if (!settings) return PLACEHOLDER[tier];
    const id = settings.tiers[tier];
    return settings.choices.find((c) => c.id === id)?.label ?? id;
  };
}
