/**
 * The icon set.
 *
 * Hand-drawn strokes rather than an icon package: the app needs about twenty
 * glyphs, and a dependency would ship a thousand. They share one grid (24),
 * one weight (1.75), and round caps, which is what makes a row of them look
 * like a set rather than a collection.
 *
 * `currentColor` throughout, so a glyph inherits whatever the thing around it
 * is doing — active nav, a tinted chip, a disabled control — without a single
 * colour prop.
 */
export type IconName =
  | "home"
  | "projects"
  | "activity"
  | "agents"
  | "skills"
  | "teams"
  | "apps"
  | "knowledge"
  | "connections"
  | "settings"
  | "plus"
  | "search"
  | "chevronRight"
  | "chevronDown"
  | "sparkle"
  | "play"
  | "board"
  | "clock"
  | "coin"
  | "check"
  | "bell"
  | "folder"
  | "chat"
  | "research";

const P: Record<IconName, React.ReactNode> = {
  home: <path d="M3.5 10.5 12 4l8.5 6.5V19a1.5 1.5 0 0 1-1.5 1.5h-4v-6h-6v6H5A1.5 1.5 0 0 1 3.5 19z" />,
  projects: (
    <>
      <rect x="3.5" y="4.5" width="17" height="15" rx="2.5" />
      <path d="M3.5 9.5h17M9 9.5v10" />
    </>
  ),
  activity: <path d="M3.5 12.5h4l2.5-6 4 13 2.5-7h4" />,
  chat: (
    <path d="M20.5 11.5a8.5 8.5 0 0 1-12.3 7.6L4 20l1-4.1a8.5 8.5 0 1 1 15.5-4.4z" />
  ),
  research: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M15.8 15.8 20.5 20.5M8.5 11h5M11 8.5v5" />
    </>
  ),
  agents: (
    <>
      <circle cx="12" cy="8.5" r="3.5" />
      <path d="M5 20c0-3.6 3.1-5.5 7-5.5s7 1.9 7 5.5" />
    </>
  ),
  skills: (
    <>
      <path d="M12 3.5 13.9 9l5.6.3-4.4 3.6 1.5 5.4-4.6-3.1-4.6 3.1 1.5-5.4L4.5 9.3 10.1 9z" />
    </>
  ),
  teams: (
    <>
      <circle cx="8.5" cy="9" r="2.8" />
      <circle cx="16.5" cy="10.5" r="2.2" />
      <path d="M3.5 19c0-3 2.3-4.6 5-4.6s5 1.6 5 4.6M14.5 19c0-2.3 1.6-3.6 3.4-3.6 1.6 0 2.6.8 2.6.8" />
    </>
  ),
  apps: (
    <>
      <rect x="3.5" y="3.5" width="7" height="7" rx="2" />
      <rect x="13.5" y="3.5" width="7" height="7" rx="2" />
      <rect x="3.5" y="13.5" width="7" height="7" rx="2" />
      <rect x="13.5" y="13.5" width="7" height="7" rx="2" />
    </>
  ),
  knowledge: (
    <>
      <path d="M4 5.5A1.5 1.5 0 0 1 5.5 4H11v16H5.5A1.5 1.5 0 0 1 4 18.5z" />
      <path d="M20 5.5A1.5 1.5 0 0 0 18.5 4H13v16h5.5a1.5 1.5 0 0 0 1.5-1.5z" />
    </>
  ),
  connections: (
    <>
      <path d="M10 14a4.5 4.5 0 0 0 6.4.2l2.6-2.6a4.5 4.5 0 0 0-6.4-6.4L11.2 6.6" />
      <path d="M14 10a4.5 4.5 0 0 0-6.4-.2L5 12.4a4.5 4.5 0 0 0 6.4 6.4l1.4-1.4" />
    </>
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 0 1-4 0v-.1A1.6 1.6 0 0 0 9 19.4a1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 0 1 0-4h.1A1.6 1.6 0 0 0 4.6 9a1.6 1.6 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 0 0 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 0 1 0 4h-.1a1.6 1.6 0 0 0-1.5 1z" />
    </>
  ),
  plus: <path d="M12 5v14M5 12h14" />,
  search: (
    <>
      <circle cx="11" cy="11" r="6.5" />
      <path d="M20 20l-3.4-3.4" />
    </>
  ),
  chevronRight: <path d="M9.5 5.5 16 12l-6.5 6.5" />,
  chevronDown: <path d="M5.5 9.5 12 16l6.5-6.5" />,
  sparkle: (
    <>
      <path d="M12 3.5 13.4 9 19 10.4 13.4 11.8 12 17.3 10.6 11.8 5 10.4 10.6 9z" />
      <path d="M18 16.5l.7 2.3 2.3.7-2.3.7-.7 2.3-.7-2.3-2.3-.7 2.3-.7z" />
    </>
  ),
  play: <path d="M8 5.5 18 12 8 18.5z" />,
  board: (
    <>
      <rect x="3.5" y="4.5" width="5" height="15" rx="1.6" />
      <rect x="10" y="4.5" width="5" height="10" rx="1.6" />
      <rect x="16.5" y="4.5" width="4" height="13" rx="1.6" />
    </>
  ),
  clock: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5V12l3 1.8" />
    </>
  ),
  coin: (
    <>
      <circle cx="12" cy="12" r="8.5" />
      <path d="M12 7.5v9M14.5 9.8c0-1.1-1.1-1.8-2.5-1.8s-2.5.7-2.5 1.8 1 1.5 2.5 1.9 2.7.9 2.7 2.2-1.2 2-2.7 2-2.7-.8-2.7-2" />
    </>
  ),
  check: <path d="M5 12.5 10 17.5 19 7" />,
  bell: (
    <>
      <path d="M18 8.5a6 6 0 1 0-12 0c0 5-2 6.5-2 6.5h16s-2-1.5-2-6.5" />
      <path d="M13.7 19a2 2 0 0 1-3.4 0" />
    </>
  ),
  folder: <path d="M3.5 7.5A1.5 1.5 0 0 1 5 6h4l2 2.5h8a1.5 1.5 0 0 1 1.5 1.5v8A1.5 1.5 0 0 1 19 19.5H5A1.5 1.5 0 0 1 3.5 18z" />,
};

/** Glyphs drawn as areas rather than outlines, so they must not be stroked. */
const FILLED: IconName[] = ["play", "skills", "sparkle", "home"];

export function Icon({
  name,
  size = 18,
  className = "",
  strokeWidth = 1.75,
}: {
  name: IconName;
  size?: number;
  className?: string;
  strokeWidth?: number;
}) {
  const filled = FILLED.includes(name);
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill={filled ? "currentColor" : "none"}
      stroke="currentColor"
      strokeWidth={filled ? 0 : strokeWidth}
      strokeLinecap="round"
      strokeLinejoin="round"
      className={className}
      // Decorative by default: every place these are used already has a text
      // label or an aria-label on the control itself, and a screen reader
      // announcing "home icon, Home" reads the same thing twice.
      aria-hidden="true"
      focusable="false"
    >
      {P[name]}
    </svg>
  );
}
