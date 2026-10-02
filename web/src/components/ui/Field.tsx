import {
  forwardRef,
  useId,
  type InputHTMLAttributes,
  type ReactNode,
  type SelectHTMLAttributes,
  type TextareaHTMLAttributes,
} from "react";
import { ChevronDown } from "lucide-react";
import { cn } from "./cn";

/**
 * Form controls. One height, one border, one focus ring — 96 inputs used to
 * carry 60 different class strings.
 */

const CONTROL =
  "w-full rounded-md border border-border bg-panel px-2.5 text-[13px] text-fg placeholder:text-fg-subtle shadow-[var(--shadow-xs)] transition-[border-color,box-shadow] duration-[var(--dur-fast)] hover:border-border-strong focus:border-accent focus:outline-none focus:ring-2 focus:ring-[color-mix(in_oklab,var(--color-accent)_22%,transparent)] disabled:cursor-not-allowed disabled:opacity-60 aria-[invalid=true]:border-danger";

export const Input = forwardRef<HTMLInputElement, InputHTMLAttributes<HTMLInputElement> & { size?: never }>(
  function Input({ className, ...rest }, ref) {
    return <input ref={ref} className={cn(CONTROL, "h-8", className)} {...rest} />;
  },
);

export const Textarea = forwardRef<HTMLTextAreaElement, TextareaHTMLAttributes<HTMLTextAreaElement>>(
  function Textarea({ className, ...rest }, ref) {
    return <textarea ref={ref} className={cn(CONTROL, "min-h-[72px] py-1.5 leading-relaxed", className)} {...rest} />;
  },
);

/** A native select, styled. Native on purpose: it is accessible and right on every platform for free. */
export const Select = forwardRef<HTMLSelectElement, SelectHTMLAttributes<HTMLSelectElement>>(function Select(
  { className, children, ...rest },
  ref,
) {
  return (
    <span className={cn("relative inline-flex w-full", className)}>
      <select ref={ref} className={cn(CONTROL, "h-8 appearance-none pr-7")} {...rest}>
        {children}
      </select>
      <ChevronDown className="pointer-events-none absolute right-2 top-1/2 size-3.5 -translate-y-1/2 text-fg-subtle" aria-hidden />
    </span>
  );
});

export function Checkbox({
  checked,
  onChange,
  label,
  hint,
  disabled,
  className,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: ReactNode;
  hint?: ReactNode;
  disabled?: boolean;
  className?: string;
}) {
  return (
    <label className={cn("flex cursor-pointer items-start gap-2 text-[13px]", disabled && "cursor-not-allowed opacity-60", className)}>
      <input
        type="checkbox"
        className="mt-0.5 size-3.5 shrink-0 accent-[var(--color-accent)]"
        checked={checked}
        disabled={disabled}
        onChange={(e) => onChange(e.target.checked)}
      />
      <span className="min-w-0">
        <span className="text-fg">{label}</span>
        {hint && <span className="mt-0.5 block text-xs text-fg-muted">{hint}</span>}
      </span>
    </label>
  );
}

/** An on/off switch: for settings that take effect at once, where a checkbox reads as "pending save". */
export function Switch({
  checked,
  onChange,
  label,
  disabled,
}: {
  checked: boolean;
  onChange: (v: boolean) => void;
  label: string;
  disabled?: boolean;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      disabled={disabled}
      onClick={() => onChange(!checked)}
      className={cn(
        "ring-focus relative inline-flex h-[18px] w-8 shrink-0 items-center rounded-full border transition-colors duration-[var(--dur-fast)] disabled:opacity-50",
        checked ? "border-accent bg-accent" : "border-border-strong bg-panel-2",
      )}
    >
      <span
        className={cn(
          "block size-3 rounded-full shadow-[var(--shadow-xs)] transition-transform duration-[var(--dur-fast)]",
          checked ? "translate-x-[15px] bg-on-accent" : "translate-x-[2px] bg-fg-subtle",
        )}
      />
    </button>
  );
}

/** A labelled control with an optional hint and error. The label is wired to the control by id. */
export function Field({
  label,
  hint,
  error,
  children,
  className,
  htmlFor,
}: {
  label: ReactNode;
  hint?: ReactNode;
  error?: ReactNode;
  /** A render function receives the id to put on the control. */
  children: ReactNode | ((id: string) => ReactNode);
  className?: string;
  htmlFor?: string;
}) {
  const auto = useId();
  const id = htmlFor ?? auto;
  return (
    <div className={cn("flex flex-col gap-1.5", className)}>
      <label htmlFor={id} className="text-xs font-medium text-fg">
        {label}
      </label>
      {typeof children === "function" ? children(id) : children}
      {error ? (
        <p className="text-xs text-danger-fg" role="alert">
          {error}
        </p>
      ) : hint ? (
        <p className="text-xs leading-relaxed text-fg-muted">{hint}</p>
      ) : null}
    </div>
  );
}
