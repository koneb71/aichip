import { cn } from "./cn";

/**
 * An agent's (or a person's) face: their colour and initial.
 *
 * The colour is the agent's own when it has one; otherwise derived from the
 * name, so the same agent looks the same everywhere without anyone choosing.
 */

const FALLBACK = ["#4f54d6", "#7c4ddb", "#0a72b5", "#0b7a55", "#b25f05", "#cc2a4a", "#5c6676"];

export function colorFor(name: string, own?: string | null): string {
  if (own) return own;
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
  return FALLBACK[h % FALLBACK.length];
}

export function Avatar({
  name,
  color,
  size = 20,
  className,
  title,
}: {
  name: string;
  color?: string | null;
  size?: number;
  className?: string;
  title?: string;
}) {
  const initial = (name.trim()[0] ?? "?").toUpperCase();
  return (
    <span
      title={title ?? name}
      aria-hidden={title ? undefined : true}
      className={cn("inline-grid shrink-0 select-none place-items-center rounded-full font-semibold text-white", className)}
      style={{
        width: size,
        height: size,
        fontSize: Math.max(9, Math.round(size * 0.46)),
        background: colorFor(name, color),
      }}
    >
      {initial}
    </span>
  );
}
