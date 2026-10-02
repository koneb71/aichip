import { useEffect, useState } from "react";
import { api, Goal } from "../lib/api";
import { flatten } from "../lib/goals";
import { Select } from "./ui/Field";

/**
 * Which goal a card serves. Active goals only, indented as the tree is; a
 * card already pointing at an achieved or abandoned one keeps showing it.
 * Renders nothing in a workspace with no goals — an empty control is a
 * question nobody here can answer yet.
 */
export function GoalPicker({
  workspaceId,
  value,
  onChange,
  disabled,
  className,
}: {
  workspaceId: string;
  value: string | null;
  onChange: (id: string | null) => void;
  disabled?: boolean;
  className?: string;
}) {
  const [goals, setGoals] = useState<Goal[] | null>(null);
  useEffect(() => {
    let stale = false;
    api
      .goals(workspaceId)
      .then((r) => !stale && setGoals(r.goals))
      .catch(() => !stale && setGoals([]));
    return () => {
      stale = true;
    };
  }, [workspaceId]);
  if (!goals || goals.length === 0) return null;
  const rows = flatten(goals).filter((r) => r.goal.status === "active" || r.goal.id === value);
  return (
    <Select
      aria-label="Goal"
      className={className}
      value={value ?? ""}
      disabled={disabled}
      onChange={(e) => onChange(e.target.value || null)}
    >
      <option value="">No goal</option>
      {rows.map(({ goal, depth }) => (
        <option key={goal.id} value={goal.id}>
          {" ".repeat(depth)}
          {goal.title}
        </option>
      ))}
    </Select>
  );
}
