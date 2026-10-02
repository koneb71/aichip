import { forwardRef, type ButtonHTMLAttributes, type ReactNode } from "react";
import { Loader2 } from "lucide-react";
import { cn } from "./cn";

/**
 * The one button.
 *
 * Before this there were 272 distinct button class strings — ten paddings for
 * "primary" alone — which is why the app looked assembled rather than
 * designed. Five variants, two sizes, and nothing else; a screen that needs a
 * sixth look is a screen that needs rethinking, not a new string.
 */

export type ButtonVariant = "primary" | "secondary" | "ghost" | "danger" | "link";
export type ButtonSize = "xs" | "sm" | "md";

const VARIANT: Record<ButtonVariant, string> = {
  primary:
    "bg-accent text-on-accent shadow-[var(--shadow-xs)] hover:bg-[color-mix(in_oklab,var(--color-accent)_88%,black)] active:translate-y-px",
  secondary:
    "border border-border bg-panel text-fg shadow-[var(--shadow-xs)] hover:bg-panel-2 hover:border-border-strong active:translate-y-px",
  ghost: "text-fg-muted hover:bg-panel-2 hover:text-fg",
  danger:
    "border border-[color-mix(in_oklab,var(--color-danger)_35%,transparent)] bg-panel text-danger-fg hover:bg-danger-subtle active:translate-y-px",
  link: "text-accent-fg underline-offset-2 hover:underline px-0! h-auto!",
};

const SIZE: Record<ButtonSize, string> = {
  xs: "h-6 gap-1 rounded-[5px] px-2 text-[11px]",
  sm: "h-7 gap-1.5 rounded-md px-2.5 text-xs",
  md: "h-8 gap-2 rounded-md px-3 text-[13px]",
};

export interface ButtonProps extends ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: ButtonVariant;
  size?: ButtonSize;
  loading?: boolean;
  /** An icon before the label. */
  icon?: ReactNode;
  /** An icon after the label, e.g. a chevron. */
  trailing?: ReactNode;
}

export const Button = forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { variant = "secondary", size = "md", loading, icon, trailing, className, children, disabled, type, ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      // A button inside a form submits unless told otherwise; nearly every
      // button here is not a submit, so the safe default is the explicit one.
      type={type ?? "button"}
      disabled={disabled || loading}
      aria-busy={loading || undefined}
      className={cn(
        "ring-focus inline-flex shrink-0 select-none items-center [&_svg]:shrink-0 justify-center whitespace-nowrap font-medium transition-[background,border-color,color,transform] duration-[var(--dur-fast)] disabled:pointer-events-none disabled:opacity-50",
        VARIANT[variant],
        SIZE[size],
        className,
      )}
      {...rest}
    >
      {loading ? <Loader2 className="size-3.5 animate-spin" aria-hidden /> : icon}
      {children}
      {trailing}
    </button>
  );
});

/** A square button that is only an icon. `label` is required: it is the name a screen reader says. */
export const IconButton = forwardRef<
  HTMLButtonElement,
  Omit<ButtonProps, "icon" | "trailing" | "children"> & { label: string; children: ReactNode }
>(function IconButton({ label, size = "sm", variant = "ghost", className, children, ...rest }, ref) {
  const box = size === "xs" ? "size-6" : size === "sm" ? "size-7" : "size-8";
  return (
    <Button
      ref={ref}
      aria-label={label}
      title={label}
      variant={variant}
      size={size}
      className={cn(box, "px-0!", className)}
      {...rest}
    >
      {children}
    </Button>
  );
});
