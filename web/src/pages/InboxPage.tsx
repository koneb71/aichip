import { useState } from "react";
import { Link } from "react-router-dom";
import { AlarmClock, ArrowUpRight, Check, CheckCheck, X } from "lucide-react";
import { api, InboxItem } from "../lib/api";
import { grouped, KIND_LABEL, useInbox } from "../lib/inbox";
import { useActivity } from "../lib/activity";
import { Page } from "../components/ui/Surface";
import { EmptyState, PageHeader } from "../components/ui/Layout";
import { Badge, type Tone } from "../components/ui/Badge";
import { Button, buttonClasses } from "../components/ui/Button";
import { Textarea } from "../components/ui/Field";
import { Menu } from "../components/ui/Overlay";
import { toast } from "../components/ui/Toast";
import { cn } from "../components/ui/cn";
import { parseForecastAsk, type ForecastAsk } from "../lib/forecast";

/**
 * Everything waiting on you, in one place, answerable where it stands.
 *
 * Each answer calls the same endpoint the thing's own screen does, so
 * approving a plan here and on the card are one action, not two that might
 * disagree. Things that need their own screen to answer well — a chat's
 * multi-part question, a preview recipe to read in full — open there.
 */
export default function InboxPage() {
  const { items, refresh } = useInbox();
  const activity = useActivity();
  const [showRead, setShowRead] = useState(true);

  const shown = (items ?? []).filter((i) => showRead || !i.read);
  const groups = grouped(shown);
  const after = () => {
    refresh();
    activity.refresh();
  };

  return (
    <Page>
      <PageHeader
        title="Inbox"
        description="Everything waiting on you — plans, permission prompts, agents' questions and proposals, changes to review."
        actions={
          <Button size="sm" variant="ghost" onClick={() => setShowRead((v) => !v)}>
            {showRead ? "Hide read" : "Show read"}
          </Button>
        }
      />

      {items === null ? (
        <div className="space-y-2">
          {[0, 1, 2].map((i) => (
            <div key={i} className="skeleton h-16 rounded-lg" />
          ))}
        </div>
      ) : groups.length === 0 ? (
        <EmptyState
          icon={<CheckCheck className="size-4" />}
          title="Nothing is waiting on you"
          hint="When a run needs a decision — a plan, a permission, a question — it lands here."
        />
      ) : (
        <div className="space-y-6">
          {groups.map((g) => (
            <section key={g.label}>
              <h2 className="mb-2 text-xs font-medium text-fg-muted">
                {g.label} <span className="tabular text-fg-subtle">{g.items.length}</span>
              </h2>
              <ul className="overflow-hidden rounded-lg border border-border bg-panel shadow-[var(--shadow-xs)]">
                {g.items.map((item, i) => (
                  <Row key={item.key} item={item} first={i === 0} onDone={after} />
                ))}
              </ul>
            </section>
          ))}
        </div>
      )}
    </Page>
  );
}

const TONE: Partial<Record<InboxItem["kind"], Tone>> = {
  permission: "warning",
  plan: "accent",
  team_plan: "accent",
  question: "info",
  decision: "complex",
  permission_expired: "danger",
  schema: "danger",
};

/** Actions that need words, and what to ask for. */
const NEEDS_TEXT: Record<string, { label: string; placeholder: string; required: boolean }> = {
  revise: { label: "Send back", placeholder: "What should change in the plan?", required: true },
  reject: { label: "Reject", placeholder: "Why? (optional)", required: false },
  answer: { label: "Answer", placeholder: "Your answer…", required: true },
};

const LABEL: Record<string, string> = {
  approve: "Approve",
  allow: "Allow",
  deny: "Deny",
  resume: "Resume",
  dismiss: "Dismiss",
  apply: "Apply",
  discard: "Discard",
  accept: "Accept",
};

function Row({ item, first, onDone }: { item: InboxItem; first: boolean; onDone: () => void }) {
  const [busy, setBusy] = useState<string | null>(null);
  const [writing, setWriting] = useState<string | null>(null);
  const [text, setText] = useState("");
  // Approving a start the budget wants confirmed: the numbers, and the same
  // "start anyway" the Start button offers. The proposal stays open meanwhile.
  const [forecast, setForecast] = useState<ForecastAsk | null>(null);

  const act = async (action: string, words?: string, acknowledgeForecast = false) => {
    setBusy(action);
    try {
      await api.resolveInbox(item.key, action, words, acknowledgeForecast);
      toast(`${LABEL[action] ?? NEEDS_TEXT[action]?.label ?? action}: ${item.title}`, { tone: "success" });
      setWriting(null);
      setForecast(null);
      onDone();
    } catch (e) {
      const ask = parseForecastAsk(String(e));
      if (ask) setForecast(ask);
      else toast("That did not go through", { tone: "danger", body: String(e).replace(/^Error:\s*/, "") });
    } finally {
      setBusy(null);
    }
  };

  const snooze = async (hours: number) => {
    try {
      await api.snoozeInbox(item.key, hours);
      onDone();
    } catch (e) {
      toast("Could not snooze", { tone: "danger", body: String(e) });
    }
  };

  const primary = item.actions.filter((a) => !NEEDS_TEXT[a] && a !== "dismiss" && a !== "deny" && a !== "discard");
  const quiet = item.actions.filter((a) => a === "deny" || a === "discard" || a === "dismiss");
  const wordy = item.actions.filter((a) => NEEDS_TEXT[a]);

  return (
    <li className={cn("px-4 py-3", !first && "border-t border-border", !item.read && "bg-[color-mix(in_oklab,var(--color-accent-subtle)_35%,transparent)]")}>
      <div className="flex items-start gap-3">
        {!item.read && <span className="mt-1.5 size-1.5 shrink-0 rounded-full bg-accent" aria-label="unread" />}
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge tone={TONE[item.kind] ?? "neutral"}>{KIND_LABEL[item.kind]}</Badge>
            {item.projectName && <span className="text-xs text-fg-muted">{item.projectName}</span>}
            <span className="text-xs text-fg-subtle">· {ago(item.createdAt)}</span>
          </div>
          <div className="mt-1 text-[13px] font-medium leading-snug text-fg">{item.title}</div>
          {item.detail && <p className="mt-0.5 line-clamp-2 break-words text-xs leading-relaxed text-fg-muted">{item.detail}</p>}

          {item.options.length > 0 && !writing && (
            <div className="mt-2 flex flex-wrap gap-1.5">
              {item.options.map((o) => (
                <Button key={o} size="xs" variant="secondary" loading={busy === "answer"} onClick={() => act("answer", o)}>
                  {o}
                </Button>
              ))}
            </div>
          )}

          {forecast && (
            <div className="mt-2 rounded-md border border-warning/40 bg-warning-subtle px-3 py-2 text-xs text-warning-fg">
              <p className="leading-relaxed">{forecast.message}</p>
              <div className="mt-2 flex gap-2">
                <Button size="sm" variant="primary" loading={busy === "approve"} onClick={() => act("approve", undefined, true)}>
                  Start anyway
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setForecast(null)}>
                  Not now
                </Button>
              </div>
            </div>
          )}

          {writing && (
            <form
              className="mt-2 flex flex-col gap-2"
              onSubmit={(e) => {
                e.preventDefault();
                if (NEEDS_TEXT[writing].required && !text.trim()) return;
                void act(writing, text);
              }}
            >
              <Textarea autoFocus value={text} onChange={(e) => setText(e.target.value)} placeholder={NEEDS_TEXT[writing].placeholder} className="min-h-[60px]" />
              <div className="flex gap-2">
                <Button type="submit" size="sm" variant="primary" loading={busy === writing} disabled={NEEDS_TEXT[writing].required && !text.trim()}>
                  {NEEDS_TEXT[writing].label}
                </Button>
                <Button size="sm" variant="ghost" onClick={() => setWriting(null)}>
                  Cancel
                </Button>
              </div>
            </form>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center justify-end gap-1.5">
          {primary.map((a) => (
            <Button key={a} size="sm" variant="primary" icon={<Check className="size-3.5" />} loading={busy === a} disabled={busy !== null} onClick={() => act(a)}>
              {LABEL[a] ?? a}
            </Button>
          ))}
          {wordy.map((a) => (
            <Button key={a} size="sm" variant={a === "answer" ? "primary" : "secondary"} disabled={busy !== null} onClick={() => setWriting(a)}>
              {NEEDS_TEXT[a].label}
            </Button>
          ))}
          {quiet.map((a) => (
            <Button key={a} size="sm" variant="ghost" icon={<X className="size-3.5" />} loading={busy === a} disabled={busy !== null} onClick={() => act(a)}>
              {LABEL[a] ?? a}
            </Button>
          ))}
          <Link
            to={item.link}
            onClick={() => void api.readInbox(item.key).catch(() => {})}
            className={buttonClasses({ size: "sm", variant: "ghost" })}
          >
            Open
            <ArrowUpRight className="size-3.5" />
          </Link>
          <Menu
            align="end"
            trigger={
              <Button size="sm" variant="ghost" aria-label="Snooze" title="Snooze">
                <AlarmClock className="size-3.5" />
              </Button>
            }
            label="Snooze until"
            items={[
              { label: "In an hour", onSelect: () => void snooze(1) },
              { label: "In 4 hours", onSelect: () => void snooze(4) },
              { label: "Tomorrow", onSelect: () => void snooze(24) },
              { label: "Next week", onSelect: () => void snooze(168) },
            ]}
          />
        </div>
      </div>
    </li>
  );
}

function ago(iso: string): string {
  const mins = Math.max(0, Math.round((Date.now() - new Date(iso).getTime()) / 60000));
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  if (mins < 60 * 24) return `${Math.round(mins / 60)}h ago`;
  return `${Math.round(mins / 1440)}d ago`;
}

