import { createContext, useContext, useEffect, useState } from "react";

/**
 * Which agent CLIs this machine actually has.
 *
 * Fetched once and shared, because every picker needs the same answer and
 * the answer only changes when the server restarts. Crucially it is *not* a
 * list baked into the bundle: offering an engine the user hasn't installed
 * produces a run that dies at spawn time, long after the choice was made.
 */
export interface EngineCapabilities {
  interactive_permissions: boolean;
  structured_rate_limit: boolean;
  resume_sessions: boolean;
  append_system_prompt: boolean;
  fixed_model_catalog: boolean;
  /** Prices its runs. False: counted by tokens only. */
  reports_cost: boolean;
  /** Holds a pass to read-only. Required of a reviewer's engine. */
  enforces_denied_tools: boolean;
  /** Can be handed Eren's tools for one run. Required by the chat
   *  assistant, a project manager and a team. */
  mcp_tools: boolean;
  /** Can edit without also being handed a shell. */
  auto_edit: boolean;
}

export interface EngineDescriptor {
  id: string;
  label: string;
  version: string | null;
  authenticated: boolean;
  providers: { name: string; auth: string }[];
  capabilities: EngineCapabilities;
}

interface EnginesState {
  engines: EngineDescriptor[] | null;
  /** The engine the server resolves when a picker is left on its inherit
   *  option. Asked of the server rather than guessed here, because which one
   *  it picks depends on what this machine has installed. */
  defaultId: string | null;
}

const Ctx = createContext<EnginesState>({ engines: null, defaultId: null });

export function EnginesProvider({ children }: { children: React.ReactNode }) {
  const [state, setState] = useState<EnginesState>({ engines: null, defaultId: null });
  useEffect(() => {
    fetch("/api/engines")
      .then((r) => r.json())
      .then((d) =>
        setState({
          engines: d.engines ?? [],
          // An older server sends no `default`; that reads as "unknown".
          defaultId: typeof d.default === "string" ? d.default : null,
        }),
      )
      .catch(() => setState({ engines: [], defaultId: null }));
  }, []);
  return <Ctx.Provider value={state}>{children}</Ctx.Provider>;
}

/** `null` until the fetch lands — distinct from "none installed". */
export function useEngines(): EngineDescriptor[] | null {
  return useContext(Ctx).engines;
}

/** The machine's default engine id, or null while unknown. */
export function useDefaultEngine(): string | null {
  return useContext(Ctx).defaultId;
}

export function useEngine(id: string | null | undefined): EngineDescriptor | undefined {
  return useContext(Ctx).engines?.find((e) => e.id === id);
}

/**
 * Why this engine can't run in this permission mode, or null if it can.
 *
 * The UI disables the option and says this, rather than letting the server
 * refuse after the click — the answer is knowable before the user commits.
 */
export function permissionBlocker(
  engine: EngineDescriptor | undefined,
  mode: string,
): string | null {
  if (!engine) return null;
  if (mode === "reviewed" && !engine.capabilities.interactive_permissions) {
    return `${engine.label} can't stop to ask you mid-run — headless it rejects every prompt instead.`;
  }
  if (mode === "auto_edit" && engine.capabilities.auto_edit === false) {
    return `${engine.label} has no setting that allows edits without also allowing commands.`;
  }
  return null;
}

/**
 * Why this engine can't do work that lives on Eren's tools, or null —
 * naming the installed engines that can, so the advice is never one this
 * machine doesn't have.
 */
export function toolsBlocker(
  engine: EngineDescriptor | undefined,
  what: string,
  installed: EngineDescriptor[] = [],
): string | null {
  if (!engine || engine.capabilities.mcp_tools !== false) return null;
  const can = installed.filter((e) => e.capabilities.mcp_tools).map((e) => e.label);
  return (
    `${engine.label} can't be handed Eren's tools for one run, and ${what} works through them.` +
    (can.length ? ` Pick ${can.join(", ")}.` : "")
  );
}

/**
 * The tools refusal, said under a picker before anyone clicks Start.
 *
 * `inheritsDefault`: the picker's inherit option falls back to the machine's
 * default engine, so a null `engine` is judged as that one. Leave it off where
 * inheriting resolves elsewhere (a team's "whatever the card says") — the
 * machine default would be the wrong engine to warn about.
 */
export function ToolsNote({
  engine,
  what,
  inheritsDefault = false,
}: {
  engine: string | null | undefined;
  what: string;
  inheritsDefault?: boolean;
}) {
  const installed = useEngines() ?? [];
  const defaultId = useDefaultEngine();
  const id = engine ?? (inheritsDefault ? defaultId : null);
  const blocker = toolsBlocker(
    installed.find((e) => e.id === id),
    what,
    installed,
  );
  if (!blocker) return null;
  return (
    <p className="mt-1.5 rounded-md border border-warning/40 bg-warning-subtle px-2 py-1 text-[11px] text-warning-fg">
      {blocker}
    </p>
  );
}

/**
 * Engine picker. Renders nothing when there's only one engine installed:
 * a choice of one is noise, and this is the common case.
 *
 * `value` of `null` means "inherit" wherever inheriting is meaningful.
 */
export function EnginePicker({
  value,
  onChange,
  inheritLabel,
  className = "",
}: {
  value: string | null;
  onChange: (id: string | null) => void;
  /** When given, an "inherit" option is offered with this label. */
  inheritLabel?: string;
  className?: string;
}) {
  const engines = useEngines();
  if (!engines || engines.length < 2) return null;
  return (
    <select
      value={value ?? ""}
      onChange={(e) => onChange(e.target.value || null)}
      className={`rounded-lg border border-border bg-panel px-2.5 py-1.5 text-xs ${className}`}
    >
      {inheritLabel && <option value="">{inheritLabel}</option>}
      {engines.map((e) => (
        <option key={e.id} value={e.id}>
          {e.label}
        </option>
      ))}
    </select>
  );
}
