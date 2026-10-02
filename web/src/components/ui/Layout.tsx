import { ChevronRight } from "lucide-react";
import type { ReactNode } from "react";
import { Link } from "react-router-dom";
import { cn } from "./cn";

/**
 * Page furniture: the header every page opens with, breadcrumbs, a toolbar
 * row, a progress bar and a meter, a table, and the empty state.
 */

export function PageHeader({
  title,
  description,
  actions,
  icon,
  meta,
  className,
}: {
  title: ReactNode;
  description?: ReactNode;
  actions?: ReactNode;
  icon?: ReactNode;
  /** Small facts under the title: counts, a status, a branch. */
  meta?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("mb-6 flex flex-wrap items-start justify-between gap-x-6 gap-y-3", className)}>
      <div className="flex min-w-0 items-start gap-3">
        {icon && <div className="mt-0.5 shrink-0">{icon}</div>}
        <div className="min-w-0">
          <h1 className="truncate text-xl font-semibold leading-tight tracking-tight text-fg">{title}</h1>
          {description && <p className="mt-1 max-w-2xl text-[13px] leading-relaxed text-fg-muted">{description}</p>}
          {meta && <div className="mt-2 flex flex-wrap items-center gap-2 text-xs text-fg-muted">{meta}</div>}
        </div>
      </div>
      {actions && <div className="flex shrink-0 flex-wrap items-center gap-2">{actions}</div>}
    </div>
  );
}

export interface Crumb {
  label: ReactNode;
  to?: string;
}

export function Breadcrumbs({ items, className }: { items: Crumb[]; className?: string }) {
  return (
    <nav aria-label="Breadcrumb" className={cn("flex min-w-0 items-center gap-1 text-[13px]", className)}>
      {items.map((c, i) => {
        const last = i === items.length - 1;
        return (
          <span key={i} className="flex min-w-0 items-center gap-1">
            {i > 0 && <ChevronRight className="size-3.5 shrink-0 text-fg-subtle" aria-hidden />}
            {c.to && !last ? (
              <Link to={c.to} className="ring-focus truncate rounded px-1 text-fg-muted hover:text-fg">
                {c.label}
              </Link>
            ) : (
              <span className={cn("truncate px-1", last ? "font-medium text-fg" : "text-fg-muted")} aria-current={last ? "page" : undefined}>
                {c.label}
              </span>
            )}
          </span>
        );
      })}
    </nav>
  );
}

export function Toolbar({ children, className }: { children: ReactNode; className?: string }) {
  return <div className={cn("mb-4 flex flex-wrap items-center gap-2", className)}>{children}</div>;
}

/** A thin bar for "how far along": goal progress, a run's steps. */
export function Progress({
  value,
  max = 1,
  tone = "accent",
  className,
  label,
}: {
  value: number;
  max?: number;
  tone?: "accent" | "success" | "warning" | "danger";
  className?: string;
  label?: string;
}) {
  const pct = max > 0 ? Math.max(0, Math.min(100, (value / max) * 100)) : 0;
  const fill = { accent: "bg-accent", success: "bg-success", warning: "bg-warning", danger: "bg-danger" }[tone];
  return (
    <div
      role="progressbar"
      aria-label={label}
      aria-valuenow={Math.round(pct)}
      aria-valuemin={0}
      aria-valuemax={100}
      className={cn("h-1.5 w-full overflow-hidden rounded-full bg-panel-2", className)}
    >
      <div className={cn("h-full rounded-full transition-[width] duration-[var(--dur-slow)]", fill)} style={{ width: `${pct}%` }} />
    </div>
  );
}

/** A meter that turns warning past `warnAt` and danger at the cap: budgets, limits. */
export function Meter({ used, cap, warnAt = 0.8, label }: { used: number; cap: number; warnAt?: number; label?: string }) {
  const ratio = cap > 0 ? used / cap : 0;
  return <Progress value={used} max={cap} label={label} tone={ratio >= 1 ? "danger" : ratio >= warnAt ? "warning" : "accent"} />;
}

export function Table({ children, className }: { children: ReactNode; className?: string }) {
  return (
    <div className={cn("overflow-x-auto rounded-lg border border-border bg-panel", className)}>
      <table className="w-full border-collapse text-[13px] [&_td]:border-t [&_td]:border-border [&_td]:px-3 [&_td]:py-2 [&_th]:bg-panel-2 [&_th]:px-3 [&_th]:py-2 [&_th]:text-left [&_th]:text-xs [&_th]:font-medium [&_th]:text-fg-muted [&_tr:hover_td]:bg-[color-mix(in_oklab,var(--color-panel-2)_55%,transparent)]">
        {children}
      </table>
    </div>
  );
}

export function EmptyState({
  icon,
  title,
  hint,
  action,
  className,
}: {
  icon?: ReactNode;
  title: ReactNode;
  hint?: ReactNode;
  action?: ReactNode;
  className?: string;
}) {
  return (
    <div className={cn("flex flex-col items-center rounded-lg border border-dashed border-border px-6 py-10 text-center", className)}>
      {icon && <div className="mb-3 grid size-9 place-items-center rounded-lg bg-panel-2 text-fg-muted">{icon}</div>}
      <div className="text-[13px] font-medium text-fg">{title}</div>
      {hint && <p className="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-fg-muted">{hint}</p>}
      {action && <div className="mt-4">{action}</div>}
    </div>
  );
}
