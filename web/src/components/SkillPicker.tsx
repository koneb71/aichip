import { Skill } from "../lib/api";

/**
 * How this card gets done.
 *
 * Sits beside the assignee rather than inside it: an agent is *who* does the
 * work, a skill is *how* this kind of job is done here, and unlike an agent and
 * a team they are not alternatives — they compose.
 *
 * Disabled skills are not offered. They stay listed on the Skills page, because
 * switching one off is the documented way to check whether it is what is
 * steering a run wrongly, and that is only useful if it comes back.
 */
export function SkillPicker({
  value,
  skills,
  disabled,
  disabledReason,
  onChange,
}: {
  value: string | null;
  skills: Skill[];
  disabled?: boolean;
  disabledReason?: string;
  onChange: (next: string | null) => void;
}) {
  // A card can point at a skill that has since been switched off. Keep it in
  // the list — dropping it would silently reset the card to "the usual way".
  const chosen = value ? skills.find((s) => s.id === value) : undefined;
  const offered = skills.filter((s) => s.enabled || s.id === value);

  return (
    <div>
      <select
        value={value ?? ""}
        disabled={disabled}
        onChange={(e) => onChange(e.target.value || null)}
        className="w-full rounded-lg border border-line bg-panel px-2 py-2 text-sm disabled:opacity-50"
      >
        <option value="">The usual way</option>
        {offered.map((s) => (
          <option key={s.id} value={s.id}>
            {s.name}
            {s.enabled ? "" : " (off)"}
          </option>
        ))}
      </select>
      {disabled && disabledReason ? (
        <div className="mt-1 text-[11px] text-ink-dim">{disabledReason}</div>
      ) : (
        chosen && (
          <div className="mt-1 text-[11px] text-ink-dim">
            {chosen.enabled
              ? chosen.description || "Its instructions go into this card's prompt."
              : "Switched off — it contributes nothing until you turn it back on."}
          </div>
        )
      )}
    </div>
  );
}
