import { Effort } from "../lib/api";

/**
 * How hard the model thinks.
 *
 * Separate from the model itself because they answer different questions —
 * which brain, and how long it gets to use it — and because the cost of getting
 * them wrong differs. Picking too small a model gives you a worse answer; too
 * much effort gives you the same answer several minutes and several times the
 * spend later.
 *
 * "Default" is a first-class option, not an empty state. It means *inherit* —
 * the bound agent's budget if it has one, otherwise the machine setting,
 * otherwise whatever the CLI does on its own. Resolved when the run starts, so
 * raising the default reaches work already sitting in the backlog.
 */
export const EFFORTS: { id: Effort; label: string }[] = [
  { id: "low", label: "Low" },
  { id: "medium", label: "Medium" },
  { id: "high", label: "High" },
  { id: "xhigh", label: "Extra high" },
  { id: "max", label: "Maximum" },
];

export function EffortPicker({
  value,
  onChange,
  disabled,
  /** What "Default" resolves to right now, when that is worth saying. */
  inherited,
  className = "",
}: {
  value: Effort | null;
  onChange: (next: Effort | null) => void;
  disabled?: boolean;
  inherited?: string | null;
  className?: string;
}) {
  return (
    <select
      value={value ?? ""}
      disabled={disabled}
      onChange={(e) => onChange((e.target.value || null) as Effort | null)}
      className={`rounded-lg border border-line bg-panel px-2 py-1 text-xs disabled:opacity-50 ${className}`}
    >
      <option value="">
        {inherited ? `Default (${inherited})` : "Default thinking"}
      </option>
      {EFFORTS.map((e) => (
        <option key={e.id} value={e.id}>
          {e.label}
        </option>
      ))}
    </select>
  );
}
