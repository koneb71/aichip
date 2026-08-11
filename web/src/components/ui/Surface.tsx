import { motion } from "framer-motion";
import { Link } from "react-router-dom";
import { itemVariants, listVariants, pageVariants } from "../../lib/motion";

/**
 * The handful of shapes every page is built from.
 *
 * Kept together because the point of them is agreement: a card here and a card
 * three pages away should have the same radius, the same border, and the same
 * idea of what hovering means. When those drift the app stops looking designed
 * and starts looking assembled.
 */

/** Soft-tinted icon chip. One hue per kind of thing. */
export type Tint = "indigo" | "violet" | "sky" | "mint" | "amber" | "rose" | "slate";

const TINT: Record<Tint, { bg: string; fg: string }> = {
  indigo: { bg: "var(--color-tint-indigo)", fg: "var(--color-ink-indigo)" },
  violet: { bg: "var(--color-tint-violet)", fg: "var(--color-ink-violet)" },
  sky: { bg: "var(--color-tint-sky)", fg: "var(--color-ink-sky)" },
  mint: { bg: "var(--color-tint-mint)", fg: "var(--color-ink-mint)" },
  amber: { bg: "var(--color-tint-amber)", fg: "var(--color-ink-amber)" },
  rose: { bg: "var(--color-tint-rose)", fg: "var(--color-ink-rose)" },
  slate: { bg: "var(--color-tint-slate)", fg: "var(--color-ink-slate)" },
};

export function TintIcon({
  tint,
  size = 44,
  children,
  className = "",
}: {
  tint: Tint;
  size?: number;
  children: React.ReactNode;
  className?: string;
}) {
  const c = TINT[tint];
  return (
    <span
      className={`grid shrink-0 place-items-center rounded-[14px] ${className}`}
      style={{ width: size, height: size, background: c.bg, color: c.fg }}
    >
      {children}
    </span>
  );
}

/** The deterministic gradient a project or space gets for a thumbnail.
 *
 * Derived from the name so the same project keeps the same colours between
 * sessions and between machines — a thumbnail that reshuffles on every reload
 * is worse than no thumbnail, because you stop using it to find things. */
export function gradientFor(seed: string): string {
  let h = 0;
  for (let i = 0; i < seed.length; i++) h = (h * 31 + seed.charCodeAt(i)) >>> 0;
  const a = h % 360;
  const b = (a + 40 + (h % 60)) % 360;
  return `linear-gradient(135deg, hsl(${a} 78% 62%), hsl(${b} 72% 52%))`;
}

/** Page wrapper: scroll container plus the entrance every route shares. */
export function Page({
  children,
  className = "",
  wide = false,
}: {
  children: React.ReactNode;
  className?: string;
  /** Full-bleed rather than the reading-width column. */
  wide?: boolean;
}) {
  return (
    <motion.div
      variants={pageVariants}
      initial="hidden"
      animate="show"
      className={`h-full overflow-y-auto ${className}`}
    >
      <div className={`${wide ? "" : "mx-auto max-w-6xl"} px-5 py-7 sm:px-8 sm:py-10`}>
        {children}
      </div>
    </motion.div>
  );
}

/** A page's title block. */
export function PageHead({
  title,
  subtitle,
  actions,
}: {
  title: React.ReactNode;
  subtitle?: React.ReactNode;
  actions?: React.ReactNode;
}) {
  return (
    <motion.div
      variants={itemVariants}
      initial="hidden"
      animate="show"
      className="mb-7 flex flex-wrap items-start justify-between gap-4"
    >
      <div className="min-w-0">
        <h1 className="text-[26px] font-bold leading-tight tracking-tight">{title}</h1>
        {subtitle && (
          <p className="mt-1.5 max-w-2xl text-sm leading-relaxed text-ink-dim">{subtitle}</p>
        )}
      </div>
      {actions && <div className="flex shrink-0 items-center gap-2">{actions}</div>}
    </motion.div>
  );
}

/** A small caps label above a group. */
export function SectionLabel({
  children,
  action,
}: {
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div className="mb-3 flex items-center justify-between gap-3">
      <h2 className="text-[11px] font-semibold uppercase tracking-[0.08em] text-ink-dim">
        {children}
      </h2>
      {action}
    </div>
  );
}

/** A staggered group. Children should be `<Item>` or any motion element. */
export function Stagger({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <motion.div variants={listVariants} initial="hidden" animate="show" className={className}>
      {children}
    </motion.div>
  );
}

/** One member of a `<Stagger>`. */
export function Item({
  children,
  className = "",
}: {
  children: React.ReactNode;
  className?: string;
}) {
  return (
    <motion.div variants={itemVariants} className={className}>
      {children}
    </motion.div>
  );
}

/** The standard card. `to` makes the whole thing a link without nesting one. */
export function Card({
  children,
  className = "",
  to,
  onClick,
  interactive,
}: {
  children: React.ReactNode;
  className?: string;
  to?: string;
  onClick?: () => void;
  interactive?: boolean;
}) {
  const cls = `group relative rounded-2xl border border-line bg-panel card-shadow ${
    to || onClick || interactive ? "lift hover:border-ink-dim/30 ring-focus" : ""
  } ${className}`;
  if (to) {
    return (
      <Link to={to} className={`block ${cls}`}>
        {children}
      </Link>
    );
  }
  if (onClick) {
    return (
      <button onClick={onClick} className={`w-full text-left ${cls}`}>
        {children}
      </button>
    );
  }
  return <div className={cls}>{children}</div>;
}

/** Nothing here yet, said without making it feel like a failure. */
export function Empty({
  icon,
  title,
  hint,
  action,
}: {
  icon?: React.ReactNode;
  title: string;
  hint?: string;
  action?: React.ReactNode;
}) {
  return (
    <motion.div
      variants={itemVariants}
      className="rounded-2xl border border-dashed border-line px-6 py-12 text-center"
    >
      {icon && <div className="mb-3 flex justify-center opacity-60">{icon}</div>}
      <div className="text-sm font-medium">{title}</div>
      {hint && <p className="mx-auto mt-1 max-w-sm text-xs leading-relaxed text-ink-dim">{hint}</p>}
      {action && <div className="mt-4 flex justify-center">{action}</div>}
    </motion.div>
  );
}

/** Placeholder blocks with a shimmer, for a page that is still fetching. */
export function Skeleton({ className = "" }: { className?: string }) {
  return <div className={`skeleton rounded-xl ${className}`} />;
}
