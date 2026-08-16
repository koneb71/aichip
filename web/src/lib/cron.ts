/**
 * Turning a schedule into fields a person can set, and back.
 *
 * Lifted out of RoutinesPage when the project manager needed the same picker.
 * Two copies of `recognize` would be the bad kind of duplication — it is a
 * parser, and a parser that disagrees with the one that wrote the string
 * silently reopens every saved schedule in the wrong tab.
 *
 * These only ever describe the *shapes the builder itself writes*. Anything
 * else is `custom`, shown as raw cron, and that is deliberate: guessing at
 * prose for an arbitrary expression is how a UI ends up confidently saying
 * "every day at 09:00" about an expression with a step in its day field.
 *
 * The authority on when a schedule actually fires is always the server —
 * `api.routinePreview` runs the same croner the scheduler does. Nothing here
 * computes a next-fire time.
 */

export type Preset = "hourly" | "daily" | "weekdays" | "weekly" | "monthly" | "custom";

export const WEEKDAYS = [
  "Sunday",
  "Monday",
  "Tuesday",
  "Wednesday",
  "Thursday",
  "Friday",
  "Saturday",
];

export interface Fields {
  preset: Preset;
  time: string;
  weekday: number;
  monthday: number;
}

/** Compile the builder's fields into five-field cron. */
export function compile(
  preset: Preset,
  time: string,
  weekday: number,
  monthday: number,
  custom: string,
): string {
  const [h, m] = time.split(":").map((n) => parseInt(n, 10));
  switch (preset) {
    case "hourly":
      return `${isNaN(m) ? 0 : m} * * * *`;
    case "daily":
      return `${m} ${h} * * *`;
    case "weekdays":
      return `${m} ${h} * * 1-5`;
    case "weekly":
      return `${m} ${h} * * ${weekday}`;
    case "monthly":
      return `${m} ${h} ${monthday} * *`;
    case "custom":
      return custom;
  }
}

/** Recognize the shapes the builder writes, so editing round-trips. */
export function recognize(expr: string): Fields {
  const m = expr.trim().match(/^(\d{1,2}) (\d{1,2}|\*) (\d{1,2}|\*) \* (\*|1-5|\d)$/);
  const fallback: Fields = { preset: "custom", time: "09:00", weekday: 1, monthday: 1 };
  if (!m) return fallback;
  const [, min, hour, dom, dow] = m;
  const time = hour === "*" ? "09:00" : `${hour.padStart(2, "0")}:${min.padStart(2, "0")}`;
  if (hour === "*" && dom === "*" && dow === "*") return { ...fallback, preset: "hourly" };
  if (dom === "*" && dow === "*") return { ...fallback, preset: "daily", time };
  if (dom === "*" && dow === "1-5") return { ...fallback, preset: "weekdays", time };
  if (dom === "*" && /^\d$/.test(dow))
    return { ...fallback, preset: "weekly", time, weekday: parseInt(dow, 10) };
  if (dom !== "*" && dow === "*")
    return { ...fallback, preset: "monthly", time, monthday: parseInt(dom, 10) };
  return fallback;
}

/** The schedule in words. */
export function describeCron(expr: string): string {
  const r = recognize(expr);
  switch (r.preset) {
    case "hourly":
      return "every hour";
    case "daily":
      return `every day at ${r.time}`;
    case "weekdays":
      return `weekdays at ${r.time}`;
    case "weekly":
      return `${WEEKDAYS[r.weekday]}s at ${r.time}`;
    case "monthly":
      return `monthly on day ${r.monthday} at ${r.time}`;
    case "custom":
      return expr;
  }
}

/** "in 4 h", "20 min ago". */
export function relative(iso: string, now: number = Date.now()): string {
  const ms = new Date(iso).getTime() - now;
  const abs = Math.abs(ms);
  const mins = Math.round(abs / 60000);
  const text =
    mins < 1
      ? "under a minute"
      : mins < 60
        ? `${mins} min`
        : mins < 60 * 48
          ? `${Math.round(mins / 60)} h`
          : `${Math.round(mins / 1440)} days`;
  return ms >= 0 ? `in ${text}` : `${text} ago`;
}
