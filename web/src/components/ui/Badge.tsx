import type { ReactNode } from "react";
import { cn } from "./cn";

/**
 * Small labels. A tone is a meaning, not a colour: `warning` is amber in the
 * light theme and a different amber in the dark one, and both are readable on
 * their wash (lib/contrast.test.ts).
 */

export type Tone = "neutral" | "accent" | "success" | "warning" | "danger" | "info" | "easy" | "medium" | "complex";

const TONE: Record<Tone, string> = {
  neutral: "bg-panel-2 text-fg-muted border-border",
  accent: "bg-accent-subtle text-accent-fg border-[color-mix(in_oklab,var(--color-accent)_25%,transparent)]",
  success: "bg-success-subtle text-success-fg border-[color-mix(in_oklab,var(--color-success)_25%,transparent)]",
  warning: "bg-warning-subtle text-warning-fg border-[color-mix(in_oklab,var(--color-warning)_28%,transparent)]",
  danger: "bg-danger-subtle text-danger-fg border-[color-mix(in_oklab,var(--color-danger)_25%,transparent)]",
  info: "bg-info-subtle text-info-fg border-[color-mix(in_oklab,var(--color-info)_25%,transparent)]",
  easy: "bg-tier-easy-soft text-tier-easy border-transparent",
  medium: "bg-tier-medium-soft text-tier-medium border-transparent",
  complex: "bg-tier-complex-soft text-tier-complex border-transparent",
};

export function Badge({
  tone = "neutral",
  children,
  className,
  icon,
  title,
}: {
  tone?: Tone;
  children: ReactNode;
  className?: string;
  icon?: ReactNode;
  title?: string;
}) {
  return (
    <span
      title={title}
      className={cn(
        "inline-flex h-[18px] max-w-full shrink-0 items-center gap-1 truncate rounded-[4px] border px-1.5 text-[11px] font-medium leading-none",
        TONE[tone],
        className,
      )}
    >
      {icon}
      {children}
    </span>
  );
}

const DOT: Record<Tone, string> = {
  neutral: "bg-fg-subtle",
  accent: "bg-accent",
  success: "bg-success",
  warning: "bg-warning",
  danger: "bg-danger",
  info: "bg-info",
  easy: "bg-tier-easy",
  medium: "bg-tier-medium",
  complex: "bg-tier-complex",
};

/** A status dot. `pulse` for something live. */
export function StatusDot({ tone = "neutral", pulse, className, label }: { tone?: Tone; pulse?: boolean; className?: string; label?: string }) {
  return (
    <span className={cn("relative inline-flex size-2 shrink-0", className)} role={label ? "img" : undefined} aria-label={label}>
      {pulse && <span className={cn("absolute inset-0 animate-ping rounded-full opacity-60", DOT[tone])} />}
      <span className={cn("relative inline-block size-2 rounded-full", DOT[tone])} />
    </span>
  );
}

/** A keyboard key. */
export function Kbd({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <kbd
      className={cn(
        "inline-flex h-[18px] min-w-[18px] items-center justify-center rounded-[4px] border border-border bg-panel-2 px-1 font-mono text-[10px] font-medium text-fg-muted",
        className,
      )}
    >
      {children}
    </kbd>
  );
}
