import type { AppField } from "../../lib/api";
import { baseType, fieldLabel } from "../../lib/apps";

/**
 * One input, chosen by the field's declared type.
 *
 * The closed type set is what makes this a `switch` rather than a plugin
 * point: there are seven types plus `ref`, they are all here, and a manifest
 * cannot introduce an eighth.
 *
 * A computed field renders read-only and says why. Letting someone type into a
 * box whose value the next save overwrites is worse than not offering the box.
 */
export function FieldInput({
  field,
  value,
  onChange,
}: {
  field: AppField;
  value: unknown;
  onChange: (v: unknown) => void;
}) {
  const label = (
    <span className="mb-1 block text-[11px] font-semibold uppercase tracking-wide text-ink-dim">
      {fieldLabel(field)}
      {field.required && !field.computed && <span className="ml-1 text-danger">*</span>}
      {field.computed && (
        <span className="ml-2 font-normal normal-case tracking-normal">
          worked out from the other fields
        </span>
      )}
    </span>
  );

  const shared =
    "w-full rounded-lg border border-line bg-surface px-2 py-1.5 text-sm " +
    "outline-none focus:border-accent disabled:opacity-60";
  const text = value === null || value === undefined ? "" : String(value);

  if (field.computed) {
    return (
      <label className="block">
        {label}
        <input className={shared} value={text} disabled readOnly />
      </label>
    );
  }

  const type = baseType(field.type);

  if (type === "bool") {
    return (
      <label className="flex items-center gap-2 pt-5">
        <input
          type="checkbox"
          checked={value === true}
          onChange={(e) => onChange(e.target.checked)}
          className="h-4 w-4 accent-[var(--color-accent)]"
        />
        <span className="text-sm">{fieldLabel(field)}</span>
      </label>
    );
  }

  if (type === "json") {
    return (
      <label className="block">
        {label}
        <textarea
          className={`${shared} font-mono text-xs`}
          rows={4}
          value={typeof value === "string" ? value : value ? JSON.stringify(value, null, 2) : ""}
          onChange={(e) => onChange(e.target.value)}
        />
      </label>
    );
  }

  const html =
    type === "int" || type === "decimal"
      ? "number"
      : type === "date"
        ? "date"
        : type === "datetime"
          ? "datetime-local"
          : "text";

  return (
    <label className="block">
      {label}
      <input
        className={shared}
        type={html}
        // A decimal is text all the way through, so a number input must not
        // round it on the way past: step="any" stops the browser snapping to
        // whole numbers, and the value is only ever read as a string.
        step={type === "decimal" ? "any" : type === "int" ? "1" : undefined}
        value={type === "datetime" ? text.slice(0, 16) : text}
        onChange={(e) => onChange(e.target.value === "" ? null : e.target.value)}
      />
    </label>
  );
}
