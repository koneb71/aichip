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
 * Takes the engine because a tier means a different model on each one —
 * OpenCode never runs `claude-opus-5`. Most callers show a label beside
 * something that hasn't chosen an engine yet, so it defaults to Claude Code
 * rather than forcing every site to invent an answer.
 *
 * Falls back to the model id when the catalog doesn't name it — the normal
 * case for OpenCode, whose ids (`anthropic/claude-sonnet-4-5`) are already
 * the most informative thing we could show.
 */
export function useTierModel(): (tier: Tier, engine?: string) => string {
  const settings = useContext(Ctx);
  return (tier: Tier, engine = "claude-code") => {
    const e = settings?.engines.find((x) => x.id === engine) ?? settings?.engines[0];
    if (!e) return PLACEHOLDER[tier];
    const id = e.tiers[tier];
    return e.choices.find((c) => c.id === id)?.label ?? id;
  };
}
