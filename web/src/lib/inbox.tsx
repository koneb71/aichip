import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import { api, InboxItem, InboxKind } from "./api";
import { useWorkspace } from "./workspace";
import { notificationsOn, notify } from "./activity";

/**
 * One poll of `/api/inbox`, shared by the sidebar count, the top bar's bell
 * and the inbox page — the same reason `lib/activity` is one poll: three
 * timers would disagree with each other for seconds at a time.
 *
 * It also announces new arrivals as browser notifications, for the people
 * who turned those on; the inbox is where everything that waits on a person
 * now lives, so it is the one place that needs to knock.
 */

interface InboxState {
  items: InboxItem[] | null;
  unread: number;
  refresh: () => void;
}

const Ctx = createContext<InboxState>({ items: null, unread: 0, refresh: () => {} });

export function InboxProvider({ children }: { children: ReactNode }) {
  const { active } = useWorkspace();
  const [items, setItems] = useState<InboxItem[] | null>(null);
  const [unread, setUnread] = useState(0);

  const refresh = useCallback(() => {
    if (!active) return;
    api
      .inbox(active.id)
      .then((r) => {
        setItems(r.items);
        setUnread(r.unread);
      })
      .catch(() => {});
  }, [active]);

  useEffect(() => {
    setItems(null);
    refresh();
    // Slower than Activity's: nothing here changes faster than a person reads.
    const timer = setInterval(() => {
      if (document.visibilityState !== "hidden") refresh();
    }, 6000);
    return () => clearInterval(timer);
  }, [refresh]);

  useAnnounce(items);

  const value = useMemo(() => ({ items, unread, refresh }), [items, unread, refresh]);
  return <Ctx.Provider value={value}>{children}</Ctx.Provider>;
}

export function useInbox(): InboxState {
  return useContext(Ctx);
}

/** The keys in `items` that `seen` did not have. Pure, for the tests. */
export function arrivals(seen: Set<string> | null, items: InboxItem[]): InboxItem[] {
  if (seen === null) return [];
  return items.filter((i) => !seen.has(i.key));
}

function useAnnounce(items: InboxItem[] | null) {
  // Seeded on the first poll, so opening the app does not announce
  // everything already waiting.
  const seen = useRef<Set<string> | null>(null);
  useEffect(() => {
    if (!items) return;
    if (notificationsOn()) {
      for (const i of arrivals(seen.current, items)) notify(KIND_LABEL[i.kind], i.title, `inbox:${i.key}`);
    }
    seen.current = new Set(items.map((i) => i.key));
  }, [items]);
}

export const KIND_LABEL: Record<InboxKind, string> = {
  plan: "Plan to review",
  team_plan: "Team plan to review",
  permission: "Permission request",
  permission_expired: "Never answered",
  question: "Question from an agent",
  decision: "Proposed decision",
  chat_question: "Question in chat",
  chat_plan: "Plan in chat",
  schema: "Schema change",
  kb_revision: "Knowledge edit",
  recipe: "Preview recipe",
};

/** The groups the inbox page shows, in order: what blocks a run first. */
export const GROUPS: { label: string; kinds: InboxKind[] }[] = [
  { label: "Blocking a run", kinds: ["permission", "plan", "team_plan"] },
  { label: "Questions", kinds: ["question", "chat_question", "chat_plan"] },
  { label: "Proposals", kinds: ["decision"] },
  { label: "Changes to review", kinds: ["schema", "kb_revision", "recipe"] },
  { label: "Cut off by a restart", kinds: ["permission_expired"] },
];

/** Items sorted into `GROUPS`, empty groups dropped. Pure, for the tests. */
export function grouped(items: InboxItem[]): { label: string; items: InboxItem[] }[] {
  return GROUPS.map((g) => ({ label: g.label, items: items.filter((i) => g.kinds.includes(i.kind)) })).filter(
    (g) => g.items.length > 0,
  );
}
