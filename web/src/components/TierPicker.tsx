import { Tier } from "../lib/api";
import { useTierModel } from "../lib/models";

/**
 * Which model, by tier rather than by name.
 *
 * The tier is the stable thing — "the cheap one", "the good one" — and the model
 * behind it is a setting that moves when a new release lands or when you switch
 * engines. Naming the resolved model in the label anyway, because "Complex" on
 * its own tells you nothing about what you are about to spend.
 *
 * The list of tiers was declared separately in four different components before
 * this existed. It is one list.
 */
export const TIERS: Tier[] = ["easy", "medium", "complex"];

const LABEL: Record<Tier, string> = {
  easy: "Easy",
  medium: "Medium",
  complex: "Complex",
};

export function TierPicker({
  value,
  onChange,
  engine,
  disabled,
  className = "",
}: {
  value: Tier;
  onChange: (next: Tier) => void;
  /** Tiers resolve to different models per engine, so the label needs this. */
  engine?: string;
  disabled?: boolean;
  className?: string;
}) {
  const tierModel = useTierModel();
  return (
    <select
      value={value}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value as Tier)}
      className={`rounded-lg border border-line bg-panel px-2 py-1 text-xs disabled:opacity-50 ${className}`}
    >
      {TIERS.map((t) => (
        <option key={t} value={t}>
          {LABEL[t]} · {tierModel(t, engine)}
        </option>
      ))}
    </select>
  );
}
