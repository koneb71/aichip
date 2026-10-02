import { motion } from "framer-motion";
import { Link } from "react-router-dom";
import { Icon, IconName } from "./ui/Icon";
import type { LucideIcon } from "lucide-react";
import { itemVariants } from "../lib/motion";
import { TintIcon, Tint } from "./ui/Surface";

/**
 * One number, large, with what it counts underneath.
 *
 * Declared twice before this existed — once on Home and once on Activity, with
 * only the mobile sizing differing, which meant the tiles on one page shrank on
 * a phone and the tiles on the other pushed the page sideways. This is the
 * version that behaves.
 *
 * `min-w-0` and the responsive type are load-bearing: a row of tiles whose
 * labels set a min-content floor is enough to make the page scroll
 * horizontally, and `max-w-*` on the container cannot claw that back.
 */
export function Stat({
  label,
  value,
  accent,
  to,
  hint,
  icon,
  tint = "slate",
}: {
  label: string;
  value: string;
  accent: string;
  /** Where this number is explained. Makes the tile a link. */
  to?: string;
  hint?: string;
  /** A lucide icon, or one of the older hand-drawn names while those remain. */
  icon?: IconName | LucideIcon;
  tint?: Tint;
}) {
  const body = (
    <div
      className={
        "card-shadow relative h-full min-w-0 overflow-hidden rounded-lg border border-border bg-panel p-3.5 " +
        (to ? "lift hover:border-border-strong" : "")
      }
    >
      {icon && (
        <TintIcon tint={tint} size={26} className="mb-2.5">
          {typeof icon === "string" ? <Icon name={icon} size={14} /> : (() => {
            const L = icon;
            return <L className="size-3.5" />;
          })()}
        </TintIcon>
      )}
      <div
        className="tabular truncate text-[22px] font-semibold leading-none tracking-tight"
        style={{ color: accent }}
      >
        {value}
      </div>
      <div className="mt-1.5 truncate text-xs text-fg-muted">{label}</div>
      {hint && <div className="mt-0.5 truncate text-[10px] text-fg-muted/80">{hint}</div>}
      {/* The arrow only appears on a tile that goes somewhere, and only under
          the pointer — a permanent one on every tile is four arrows competing
          with the numbers they sit beside. */}
      {to && (
        <span className="absolute right-3.5 top-3.5 text-fg-muted opacity-0 transition-opacity duration-200 group-hover:opacity-100">
          <Icon name="chevronRight" size={15} />
        </span>
      )}
    </div>
  );
  return (
    <motion.div variants={itemVariants} className="group min-w-0">
      {to ? (
        <Link to={to} className="ring-focus block h-full min-w-0 rounded-lg">
          {body}
        </Link>
      ) : (
        body
      )}
    </motion.div>
  );
}
