import { Agent, Team } from "../lib/api";

/**
 * Who does this card — one person, one team, or nobody yet.
 *
 * One control rather than two, because they are alternatives: a card handed
 * to a team never runs its agent, so offering both at once invites a
 * combination that silently does something other than what it shows.
 *
 * Shared between the new-task modal and the task drawer so that creating an
 * assignment and changing one look and behave the same.
 */
export type Assignee = { kind: "agent" | "team"; id: string } | null;

/** The `agent:<id>` / `team:<id>` form the <select> uses as its value. */
export function assigneeValue(a: Assignee): string {
  return a ? `${a.kind}:${a.id}` : "";
}

export function parseAssignee(value: string): Assignee {
  if (!value) return null;
  const [kind, id] = value.split(":");
  return kind === "agent" || kind === "team" ? { kind, id } : null;
}

export function AssigneePicker({
  value,
  agents,
  teams,
  disabled,
  disabledReason,
  onChange,
}: {
  value: Assignee;
  agents: Agent[];
  teams: Team[];
  disabled?: boolean;
  /** Shown instead of the usual hint when the control is locked. */
  disabledReason?: string;
  onChange: (next: Assignee) => void;
}) {
  const team = value?.kind === "team" ? teams.find((t) => t.id === value.id) : undefined;

  return (
    <div>
      <select
        value={assigneeValue(value)}
        disabled={disabled}
        onChange={(e) => onChange(parseAssignee(e.target.value))}
        className="w-full rounded-lg border border-line bg-panel px-2 py-2 text-sm disabled:opacity-50"
      >
        <option value="">Nobody in particular</option>
        {agents.length > 0 && (
          <optgroup label="Agents">
            {agents.map((a) => (
              <option key={a.id} value={`agent:${a.id}`}>
                {a.name}
              </option>
            ))}
          </optgroup>
        )}
        {teams.length > 0 && (
          <optgroup label="Teams">
            {teams.map((t) => (
              <option key={t.id} value={`team:${t.id}`}>
                {t.name} ({t.pattern})
              </option>
            ))}
          </optgroup>
        )}
      </select>
      {disabled && disabledReason ? (
        <div className="mt-1 text-[11px] text-ink-dim">{disabledReason}</div>
      ) : (
        team && (
          <div className="mt-1 text-[11px] text-ink-dim">
            {team.pattern === "org"
              ? "The manager will split this up and delegate it."
              : `Runs as a ${team.pattern}; the model tier is ignored.`}
          </div>
        )
      )}
    </div>
  );
}
